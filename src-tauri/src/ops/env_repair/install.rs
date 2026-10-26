//! 安装管线：JDK 解压安装 / MSI 静默安装 / ZIP 通用安装（MySQL/Redis）。

use std::sync::Arc;

use super::{EnvCache, RepairState};
#[cfg(target_os = "windows")]
use super::{finish, JDK_DEFAULT_MAJOR, JDK_MANAGED_HOME};
#[cfg(target_os = "windows")]
use super::jdk::{ensure_user_path_entry, find_compatible, probe_home_version, set_user_java_home};
#[cfg(target_os = "windows")]
use super::uninstall::mysql_data_initialized;


/// java 免安装版验证：主目录可探出版本且达到最低大版本要求。
#[cfg(target_os = "windows")]
fn verify_java_home(home: &str, min_major: u32) -> Result<(u32, u32, u32), String> {
    let (maj, min, pat) = probe_home_version(home).ok_or_else(|| {
        format!("解压完成但 {home}\\bin\\java.exe 无法运行或无法识别版本，请确认安装包完整")
    })?;
    if maj < min_major {
        return Err(format!("JDK 版本 {maj}.{min}.{pat} 低于要求的 {min_major}+，请更换安装包"));
    }
    Ok((maj, min, pat))
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
/// Adoptium 官方二进制 zip（免安装版）——解压即用，不写 MSI 产品/注册表；
/// 平台以「写用户级 JAVA_HOME + 平台内绝对路径调用」接管指向，卸载零残留。
const ADOPTIUM_ZIP_URL: &str =
    "https://api.adoptium.net/v3/binary/latest/{major}/ga/windows/x64/jre/hotspot/normal/eclipse";

/// 启动 JDK 自动安装（仅 Windows 目标机）。
/// 返回 Err 表示无法启动（已有任务在跑 / 平台不支持）。
#[cfg(target_os = "windows")]
pub fn start_jdk_install(
    expected_major: u32,
    env_cache: EnvCache,
    state: Arc<RepairState>,
) -> Result<(), String> {
    {
        let mut job = state.job.lock().unwrap();
        if job.running {
            return Err("已有修复任务在执行，请等待其完成".into());
        }
        job.running = true;
        job.target = Some("java".into());
        job.op = "install".into();
        job.stage = "download".into();
        job.ok = None;
        job.error = None;
        job.logs.clear();
    }
    tokio::spawn(async move {
        run_install(state, expected_major, env_cache).await;
    });
    Ok(())
}

/// 通用修复任务：本地资源目录里的 MSI 静默安装（java/mysql/redis/vernemq）。
/// 安装命令由 resource::msi_install_command 按目标软件生成（java 带
/// FeatureJavaHome/FeatureEnvironment 自动写 JAVA_HOME 与 PATH，其余 /quiet）。
/// 返回 Err 表示无法启动（已有任务在跑 / 平台不支持 / 无预置安装链路）。
#[cfg(target_os = "windows")]
pub fn start_resource_install(
    target: &'static str,
    msi_path: String,
    env_cache: EnvCache,
    state: Arc<RepairState>,
) -> Result<(), String> {
    if crate::ops::resource::msi_install_command(target, &msi_path).is_none() {
        return Err(format!(
            "{} 没有预置的静默安装链路，请使用 AI 方案或手动安装",
            crate::ops::resource::target_label(target)
        ));
    }
    {
        let mut job = state.job.lock().unwrap();
        if job.running {
            return Err("已有修复任务在执行，请等待其完成".into());
        }
        job.running = true;
        job.target = Some(target.into());
        job.op = "install".into();
        job.stage = "install".into();
        job.ok = None;
        job.error = None;
        job.logs.clear();
    }
    tokio::spawn(async move {
        run_resource_install(state, target, msi_path, env_cache).await;
    });
    Ok(())
}

/// 非 Windows 平台：dev 机不提供本地安装包安装（交付平台是 Windows）。
#[cfg(not(target_os = "windows"))]
pub fn start_resource_install(
    target: &'static str,
    _msi_path: String,
    _env_cache: EnvCache,
    _state: Arc<RepairState>,
) -> Result<(), String> {
    Err(format!(
        "本地安装包安装仅支持 Windows 目标机，{} 请手动安装",
        crate::ops::resource::target_label(target)
    ))
}

/// 非 Windows 平台：dev 机不提供自动安装（交付平台是 Windows）。
#[cfg(not(target_os = "windows"))]
pub fn start_jdk_install(
    _expected_major: u32,
    _env_cache: EnvCache,
    _state: Arc<RepairState>,
) -> Result<(), String> {
    Err("自动安装仅支持 Windows 目标机；当前平台请手动安装 Java 运行时".into())
}

/// Windows 安装流水线（在线兜底）：下载 Adoptium 免安装版 zip → 解压到平台目录
/// → 写用户级 JAVA_HOME → 版本验证。全程无 UAC、无 MSI、无注册表产品残留。
#[cfg(target_os = "windows")]
async fn run_install(state: Arc<RepairState>, expected_major: u32, env_cache: EnvCache) {
    state.log(format!(
        "开始修复：目标 JDK {expected_major}+（Eclipse Temurin 免安装版，Adoptium 官方源）"
    ));

    // ===== 1. 下载 zip =====
    state.patch(|j| j.stage = "download".into());
    let url = ADOPTIUM_ZIP_URL.replace("{major}", &expected_major.to_string());
    let zip = std::env::temp_dir().join(format!("dfy-ops-temurin{expected_major}-jre.zip"));
    let dl = async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(900))
            .build()
            .map_err(|e| format!("HTTP 客户端初始化失败：{e}"))?;
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("下载失败（网络不可达？）：{e}"))?
            .error_for_status()
            .map_err(|e| format!("下载失败：{e}"))?;
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("下载中断：{e}"))?;
        std::fs::write(&zip, &bytes).map_err(|e| format!("写入临时文件失败：{e}"))?;
        Ok(bytes.len() as u64)
    };
    match dl.await {
        Ok(size) => state.log(format!("安装包下载完成：{}（{} MB）", zip.display(), size / 1024 / 1024)),
        Err(e) => {
            finish(state, env_cache, false, e).await;
            return;
        }
    }

    // ===== 2. 解压 + 配置（免安装管线，与资源目录 zip 同链路） =====
    state.patch(|j| j.stage = "extract".into());
    state.log(format!("正在解压到 {JDK_MANAGED_HOME}…"));
    let located = tokio::task::spawn_blocking(move || {
        extract_zip(zip.to_str().unwrap_or_default(), JDK_MANAGED_HOME)
            .and_then(|()| locate_home(std::path::Path::new(JDK_MANAGED_HOME), r"bin\java.exe"))
    })
    .await;
    let home = match located {
        Ok(Ok(h)) => {
            state.log(format!("解压完成，JDK 主目录：{h}"));
            h
        }
        Ok(Err(e)) => {
            finish(state, env_cache, false, e).await;
            return;
        }
        Err(e) => {
            finish(state, env_cache, false, format!("解压任务异常：{e}")).await;
            return;
        }
    };

    state.patch(|j| j.stage = "configure".into());
    let home_for_cfg = home.clone();
    let cfg = tokio::task::spawn_blocking(move || configure_zip_target("java", &home_for_cfg)).await;
    match cfg {
        Ok(Ok(msg)) => state.log(msg),
        Ok(Err(e)) => {
            finish(state, env_cache, false, e).await;
            return;
        }
        Err(e) => {
            finish(state, env_cache, false, format!("配置任务异常：{e}")).await;
            return;
        }
    }

    // ===== 3. 版本验证（绝对路径直接探测） =====
    state.patch(|j| j.stage = "verify".into());
    let v = tokio::task::spawn_blocking(move || verify_java_home(&home, expected_major)).await;
    match v {
        Ok(Ok((maj, min, pat))) => {
            state.log(format!("验证通过：JDK {maj}.{min}.{pat} 可用，JAVA_HOME 已指向平台免安装目录"));
            state.log("提示：已开终端里的 JAVA_HOME 是旧值，重开终端后生效");
            finish(state, env_cache, true, String::new()).await;
        }
        Ok(Err(e)) => {
            finish(state, env_cache, false, e).await;
        }
        Err(e) => {
            finish(state, env_cache, false, format!("验证任务异常：{e}")).await;
        }
    }
}

/// 本地资源包安装流水线（Windows）：UAC 提权 msiexec 静默安装 → 复探验证。
/// 与 run_install 的差异：安装包来自资源目录（离线），无需下载；验证仅 java
/// 有可靠的注册表/目录扫描口径，其余目标以 msiexec 退出码为准并提示重新检测。
#[cfg(target_os = "windows")]
async fn run_resource_install(
    state: Arc<RepairState>,
    target: &'static str,
    msi_path: String,
    env_cache: EnvCache,
) {
    state.log(format!(
        "开始修复：{}（本地安装包 {}）",
        crate::ops::resource::target_label(target),
        msi_path
    ));

    state.patch(|j| j.stage = "install".into());
    // msi_install_command 返回 "msiexec /i \"path\" ..."，取首 token 之后的参数串
    // 交给 Start-Process 的 ArgumentList（无 shell 解释层，无注入面）。
    let Some(cmd) = crate::ops::resource::msi_install_command(target, &msi_path) else {
        finish(state, env_cache, false, "该目标没有预置的静默安装链路".into()).await;
        return;
    };
    let args_part = cmd["msiexec".len()..].trim().to_string();
    // PowerShell 单引号字面量转义（路径含单引号时防破坏命令结构）
    let args_part = args_part.replace('\'', "''");
    state.log("正在启动 MSI 静默安装…请在弹出的 UAC 管理员确认框中点击「是」…");
    let install = tokio::task::spawn_blocking(move || {
        let ps = if args_part.is_empty() {
            "Start-Process msiexec -Verb RunAs -Wait".to_string()
        } else {
            format!("Start-Process msiexec -ArgumentList '{args_part}' -Verb RunAs -Wait")
        };
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
    })
    .await;
    match install {
        Ok(Ok(out)) if out.status.success() => {
            state.log("MSI 安装完成");
        }
        Ok(Ok(out)) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            finish(
                state,
                env_cache,
                false,
                format!("安装执行失败（可能用户在 UAC 中点了取消）：{stderr}"),
            )
            .await;
            return;
        }
        Ok(Err(e)) => {
            finish(state, env_cache, false, format!("无法启动安装器：{e}")).await;
            return;
        }
        Err(e) => {
            finish(state, env_cache, false, format!("安装任务异常：{e}")).await;
            return;
        }
    }

    // ===== 复探验证 =====
    state.patch(|j| j.stage = "verify".into());
    if target == "java" {
        // java：注册表/目录扫描 + 绝对路径探测（新 PATH 对本进程不可见，不能靠 `java -version`）
        let found = tokio::task::spawn_blocking(move || find_compatible(8)).await.ok().flatten();
        match found {
            Some(j) => {
                state.log(format!(
                    "验证通过：检测到 JDK {}（{}）已安装并配置环境变量",
                    j.version_label(),
                    j.home
                ));
                finish(state, env_cache, true, String::new()).await;
            }
            None => {
                finish(
                    state,
                    env_cache,
                    false,
                    "安装流程已执行但未检测到达标 JDK，请查看安装日志或手动安装".into(),
                )
                .await;
            }
        }
    } else {
        // mysql/redis/vernemq：安装器返回成功即视为安装完成；服务注册/端口监听
        // 状态由「重新检查」重新实测（本进程看不到安装器写的新 PATH）
        state.log("安装流程已执行完成，请点击「重新检查」验证环境状态");
        finish(state, env_cache, true, String::new()).await;
    }
}

// ===== ZIP 免安装版多步流水线（mysql/redis） =====

/// 目标软件的 Windows 服务名与默认端口（zip 免安装版管线）。
/// 端口取服务注册表内置默认（registry::builtin 与之一致）。
pub fn service_spec(target: &str) -> Option<(&'static str, u16)> {
    match target {
        "mysql" => Some(("MySQL80", 3306)),
        "redis" => Some(("Redis", 6379)),
        _ => None,
    }
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn mysql_ini_path(home: &str) -> String {
    format!("{home}\\my.ini")
}

/// my.ini 内容：路径统一用正斜杠（MySQL 兼容），datadir 指向 home/data。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn build_mysql_ini(home: &str) -> String {
    let h = home.replace('\\', "/");
    format!(
        "[mysqld]\nbasedir={h}\ndatadir={h}/data\nport=3306\ncharacter-set-server=utf8mb4\ncollation-server=utf8mb4_unicode_ci\ndefault-storage-engine=INNODB\n"
    )
}

/// Redis 配置路径：zip 自带 redis.windows.conf 则用之，否则用平台最小配置。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn redis_conf_path(home: &str) -> String {
    let bundled = format!("{home}\\redis.windows.conf");
    if std::path::Path::new(&bundled).is_file() {
        bundled
    } else {
        format!("{home}\\redis-dfy.conf")
    }
}

/// Redis 最小配置（zip 不带 redis.windows.conf 时的兜底）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn build_redis_conf() -> String {
    "port 6379\nbind 0.0.0.0\n".to_string()
}

/// 通用修复任务：ZIP 免安装版离线安装（mysql/redis/java）——
/// mysql/redis = 解压 → 配置 → 注册服务（一次 UAC，
/// MySQL 官方 msi 多为 Installer 束静默装不上 Server，zip 才能完全离线装好）；
/// java = 解压 → 写用户级 JAVA_HOME → 版本验证（无服务、无 UAC）。
/// 流水线：解压定位 → 配置 → （服务注册）→ 验证。
/// install_dir 取服务注册表该服务的 installDir（如 C:/services/mysql），java 固定 JDK_MANAGED_HOME。
#[cfg(target_os = "windows")]
pub fn start_zip_install(
    target: &'static str,
    zip_path: String,
    install_dir: String,
    env_cache: EnvCache,
    state: Arc<RepairState>,
) -> Result<(), String> {
    if service_spec(target).is_none() && target != "java" {
        return Err(format!(
            "{} 没有免安装版（zip）安装管线，请使用 MSI 安装包或 AI 方案",
            crate::ops::resource::target_label(target)
        ));
    }
    {
        let mut job = state.job.lock().unwrap();
        if job.running {
            return Err("已有修复任务在执行，请等待其完成".into());
        }
        job.running = true;
        job.target = Some(target.into());
        job.op = "install".into();
        job.stage = "extract".into();
        job.ok = None;
        job.error = None;
        job.logs.clear();
    }
    tokio::spawn(async move {
        run_zip_install(state, target, zip_path, install_dir, env_cache).await;
    });
    Ok(())
}

/// 非 Windows：dev 机不提供免安装版管线（交付平台是 Windows）。
#[cfg(not(target_os = "windows"))]
pub fn start_zip_install(
    target: &'static str,
    _zip_path: String,
    _install_dir: String,
    _env_cache: EnvCache,
    _state: Arc<RepairState>,
) -> Result<(), String> {
    Err(format!(
        "免安装版（zip）安装仅支持 Windows 目标机，{} 请手动安装",
        crate::ops::resource::target_label(target)
    ))
}

/// PowerShell 单引号字面量转义（路径含空格/引号时保结构）。
#[cfg(target_os = "windows")]
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// UAC 提权执行单条命令（程序 + 参数串），等待完成；返回 Err = 非零退出/取消。
/// 非 Windows 恒 Err（dev 机上服务启停接口会如实报错）。
pub fn runas_wait(exe: &str, args: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let body = if args.is_empty() {
            format!(
                "$p = Start-Process -FilePath {} -Verb RunAs -Wait -PassThru; if ($p) {{ exit $p.ExitCode }} else {{ exit 1 }}",
                ps_quote(exe)
            )
        } else {
            format!(
                "$p = Start-Process -FilePath {} -ArgumentList {} -Verb RunAs -Wait -PassThru; if ($p) {{ exit $p.ExitCode }} else {{ exit 1 }}",
                ps_quote(exe),
                ps_quote(args)
            )
        };
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &body])
            .output()
            .map_err(|e| format!("无法启动 PowerShell：{e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if err.is_empty() {
                format!("命令执行失败（退出码 {:?}，可能用户在 UAC 中点了取消）", out.status.code())
            } else {
                err
            })
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (exe, args);
        Err("该操作仅支持 Windows 目标机".into())
    }
}

/// 解压 zip 到目标目录：优先系统自带 bsdtar（快），失败回退 Expand-Archive。
#[cfg(target_os = "windows")]
fn extract_zip(zip: &str, dest: &str) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("创建目标目录失败：{e}"))?;
    let tar = std::process::Command::new("tar").args(["-xf", zip, "-C", dest]).output();
    let tar_ok = matches!(&tar, Ok(o) if o.status.success());
    if !tar_ok {
        let ps = format!(
            "Expand-Archive -LiteralPath {} -DestinationPath {} -Force",
            ps_quote(zip),
            ps_quote(dest)
        );
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
            .map_err(|e| format!("无法启动解压工具：{e}"))?;
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(format!("解压失败：{err}"));
        }
    }
    Ok(())
}

/// 解压后在目标目录里定位「程序主目录」：zip 可能带顶层目录（如
/// mysql-8.0.42-winx64/），以标志二进制所在层为准。
#[cfg(target_os = "windows")]
fn locate_home(dest: &std::path::Path, marker: &str) -> Result<String, String> {
    if dest.join(marker).is_file() {
        return Ok(dest.to_string_lossy().to_string());
    }
    if let Ok(entries) = std::fs::read_dir(dest) {
        for e in entries.flatten() {
            if e.path().is_dir() && e.path().join(marker).is_file() {
                return Ok(e.path().to_string_lossy().to_string());
            }
        }
    }
    Err(format!("解压完成但未找到标志文件 {marker}，请确认安装包内容完整"))
}

/// 写配置 + 初始化数据（mysql 初始化数据目录较慢，分钟级属正常）。
#[cfg(target_os = "windows")]
fn configure_zip_target(target: &str, home: &str) -> Result<String, String> {
    match target {
        "mysql" => {
            let ini = mysql_ini_path(home);
            std::fs::write(&ini, build_mysql_ini(home))
                .map_err(|e| format!("写入 my.ini 失败：{e}"))?;
            // 卸载保留数据 → 重装复用：datadir 已初始化（含系统库）直接跳过，
            // 再跑 --initialize-insecure 会因目录非空而失败
            if mysql_data_initialized(home) {
                return Ok("检测到已有数据目录，跳过初始化（复用现有数据）".into());
            }
            let mysqld = format!("{home}\\bin\\mysqld.exe");
            let out = std::process::Command::new(&mysqld)
                // --defaults-file 必须是首个选项（mysqld 的解析要求）
                .args([format!("--defaults-file={ini}").as_str(), "--initialize-insecure", "--console"])
                .output()
                .map_err(|e| format!("无法运行 mysqld.exe：{e}"))?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                let tail: String = err.lines().rev().take(5).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join(" | ");
                return Err(format!("数据目录初始化失败：{tail}"));
            }
            Ok("已写入 my.ini 并初始化数据目录（root 空密码，装好后请尽快改密）".into())
        }
        "redis" => {
            if !std::path::Path::new(&redis_conf_path(home)).is_file() {
                std::fs::write(format!("{home}\\redis-dfy.conf"), build_redis_conf())
                    .map_err(|e| format!("写入 redis 配置失败：{e}"))?;
                Ok("已写入最小配置 redis-dfy.conf（端口 6379）".into())
            } else {
                Ok("使用安装包自带的 redis.windows.conf".into())
            }
        }
        "java" => {
            // 平台直接指向：写用户级 JAVA_HOME + 把 {home}\bin 追加进用户 PATH。
            // PATH 必须配——否则 cmd 里 `java -version` 无法识别；注册表原始读写
            // 而非 setx（避免长 PATH 被截断），写完自动广播。
            // 卸载时删除该变量并从 PATH 移除条目即可完全清理，系统零残留
            #[cfg(target_os = "windows")]
            {
                set_user_java_home(home).map_err(|e| format!("配置 JAVA_HOME 失败：{e}"))?;
                let bin = format!("{home}\\bin");
                match ensure_user_path_entry(&bin) {
                    Ok(true) => {
                        return Ok(format!(
                            "已写入用户级 JAVA_HOME={home}，并把 {bin} 加入用户 PATH（重开命令行窗口生效）"
                        ));
                    }
                    Ok(false) => {
                        return Ok(format!("已写入用户级 JAVA_HOME={home}（PATH 中已存在 {bin}，未重复添加）"));
                    }
                    Err(e) => return Err(format!("PATH 追加失败：{e}")),
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = home;
                Ok("非 Windows 平台无需配置".into())
            }
        }
        _ => Err("该目标没有配置步骤".into()),
    }
}

/// 注册并启动 Windows 服务（清理旧服务 + 安装 + 启动合并为一次 UAC 提权，
/// 避免用户连点三次确认框）。
#[cfg(target_os = "windows")]
fn register_and_start(target: &str, home: &str) -> Result<(), String> {
    let (svc, _port) = service_spec(target).unwrap();
    let script = match target {
        "mysql" => format!(
            "net stop {svc} & taskkill /f /im mysqld.exe & ping -n 3 127.0.0.1 >nul & sc delete {svc} & \"{}\" --install {svc} --defaults-file=\"{}\" & net start {svc}",
            format!("{home}\\bin\\mysqld.exe"),
            mysql_ini_path(home)
        ),
        "redis" => format!(
            "net stop {svc} & taskkill /f /im redis-server.exe & ping -n 3 127.0.0.1 >nul & sc delete {svc} & \"{}\" --service-install \"{}\" --service-name {svc} & net start {svc}",
            format!("{home}\\redis-server.exe"),
            redis_conf_path(home)
        ),
        _ => return Err("该目标没有服务注册管线".into()),
    };
    runas_wait("cmd.exe", &format!("/c {script}"))
}

#[cfg(target_os = "windows")]
async fn run_zip_install(
    state: Arc<RepairState>,
    target: &'static str,
    zip_path: String,
    install_dir: String,
    env_cache: EnvCache,
) {
    let label = crate::ops::resource::target_label(target);
    let spec = service_spec(target);
    state.log(format!("开始安装 {label}（免安装版）：{zip_path} → {install_dir}"));

    // ===== 解压 + 定位主目录 =====
    state.patch(|j| j.stage = "extract".into());
    state.log("正在解压安装包（大包需 1-2 分钟）…");
    let marker = match target {
        "mysql" => r"bin\mysqld.exe",
        "redis" => "redis-server.exe",
        // JDK/JRE zip 的标志二进制（zip 可能带顶层目录 jdk8uXXX-jre/）
        "java" => r"bin\java.exe",
        _ => {
            finish(state, env_cache, false, format!("{label} 没有免安装版（zip）安装管线")).await;
            return;
        }
    };
    let located = {
        let zp = zip_path.clone();
        let dd = install_dir.clone();
        tokio::task::spawn_blocking(move || {
            extract_zip(&zp, &dd).and_then(|()| locate_home(std::path::Path::new(&dd), marker))
        })
        .await
    };
    let home = match located {
        Ok(Ok(h)) => {
            state.log(format!("解压完成，程序目录：{h}"));
            h
        }
        Ok(Err(e)) => {
            finish(state, env_cache, false, e).await;
            return;
        }
        Err(e) => {
            finish(state, env_cache, false, format!("解压任务异常：{e}")).await;
            return;
        }
    };

    // ===== 写配置 + 初始化数据 =====
    state.patch(|j| j.stage = "configure".into());
    let cfg = {
        let home = home.clone();
        let t = target;
        tokio::task::spawn_blocking(move || configure_zip_target(t, &home)).await
    };
    match cfg {
        Ok(Ok(msg)) => state.log(msg),
        Ok(Err(e)) => {
            finish(state, env_cache, false, e).await;
            return;
        }
        Err(e) => {
            finish(state, env_cache, false, format!("配置任务异常：{e}")).await;
            return;
        }
    }

    // ===== 注册 + 启动服务（一次 UAC）——java 免安装版无服务概念，跳过 =====
    if let Some((svc, _)) = spec {
        state.patch(|j| j.stage = "install".into());
        state.log(format!("正在注册并启动 Windows 服务 {svc}… 请在 UAC 弹窗中点击「是」…"));
        let reg = {
            let home = home.clone();
            let t = target;
            tokio::task::spawn_blocking(move || register_and_start(t, &home)).await
        };
        match reg {
            Ok(Ok(())) => state.log(format!("服务 {svc} 已注册并启动")),
            Ok(Err(e)) => {
                finish(
                    state,
                    env_cache,
                    false,
                    format!("服务注册/启动失败：{e}（可到 Windows 服务管理器查看 {svc} 状态）"),
                )
                .await;
                return;
            }
            Err(e) => {
                finish(state, env_cache, false, format!("服务注册任务异常：{e}")).await;
                return;
            }
        }
    }

    // ===== 验证：mysql/redis 轮询端口监听；java 直接验证主目录版本 =====
    state.patch(|j| j.stage = "verify".into());
    let Some((svc, port)) = spec else {
        let home_for_verify = home.clone();
        let v = tokio::task::spawn_blocking(move || verify_java_home(&home_for_verify, JDK_DEFAULT_MAJOR)).await;
        match v {
            Ok(Ok((maj, min, pat))) => {
                state.log(format!("验证通过：JDK {maj}.{min}.{pat} 可用，JAVA_HOME 已指向 {home}"));
                state.log("提示：已开终端里的 JAVA_HOME 是旧值，重开终端后生效");
                finish(state, env_cache, true, String::new()).await;
            }
            Ok(Err(e)) => {
                finish(state, env_cache, false, e).await;
            }
            Err(e) => {
                finish(state, env_cache, false, format!("验证任务异常：{e}")).await;
            }
        }
        return;
    };
    state.log(format!("等待 {label} 在端口 {port} 监听…（首次启动较慢）"));
    let mut ok = false;
    for _ in 0..45 {
        if crate::ops::health::check_tcp("127.0.0.1", port, 800) {
            ok = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
    if ok {
        state.log(format!("验证通过：端口 {port} 正在监听"));
        finish(state, env_cache, true, String::new()).await;
    } else {
        finish(
            state,
            env_cache,
            false,
            format!("服务已注册但端口 {port} 未监听，请到 Windows 服务管理器检查 {svc} 启动状态或查看 MySQL 错误日志（datadir 内 *.err）"),
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// zip 免安装版管线：服务名/端口映射与配置生成。
    #[test]
    fn zip_pipeline_spec_and_configs() {
        assert_eq!(service_spec("mysql"), Some(("MySQL80", 3306)));
        assert_eq!(service_spec("redis"), Some(("Redis", 6379)));
        assert_eq!(service_spec("java"), None, "java 走 msi/在线，无 zip 管线");

        let ini = build_mysql_ini(r"C:\services\mysql");
        assert!(ini.contains("basedir=C:/services/mysql"));
        assert!(ini.contains("datadir=C:/services/mysql/data"));
        assert!(ini.contains("port=3306"));
        assert!(!ini.contains('\\'), "my.ini 路径不应含反斜杠: {ini}");

        assert_eq!(build_redis_conf(), "port 6379\nbind 0.0.0.0\n");
    }
}
