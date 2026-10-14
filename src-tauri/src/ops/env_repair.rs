//! 环境修复：Java 运行时发现与自动安装（Windows 目标机交付场景）。
//!
//! 检测口径（与 environment.rs 的「进程是事实来源」一致）：
//! - 服务由 `run_cmd`（`java -jar …`）启动，用的是 **PATH 上的 java**——
//!   因此 PATH java 是第一事实；已装 JDK 清单只在 PATH 不达标时才盘点（省探测开销）。
//! - 清单来源：Windows 读注册表约定键 + 默认安装目录兜底；macOS 用
//!   `/usr/libexec/java_home -V`（仅 dev 机参考，交付平台是 Windows）。
//!
//! 修复分级（POST /api/env/repair 的决策链）：
//! 1. PATH java 达标 → 无需修复；
//! 2. 本机已有达标 JDK → 返回配置指引（设 JAVA_HOME / 加 PATH），不擅自改；
//! 3. 纯净机什么都没有 → 自动安装：下载 Adoptium Temurin JRE MSI →
//!    UAC 提权静默安装（MSI 的 FeatureJavaHome + FeatureEnvironment 自动写
//!    JAVA_HOME 与 PATH，无需手工改注册表）→ 复探验证。

use serde::Serialize;
use std::sync::{Arc, Mutex};

/// 本机一个已安装的 Java 运行时（home 目录级）。
#[derive(Debug, Clone)]
pub struct JavaInstall {
    /// JAVA_HOME 级目录（bin/java 的上一级）
    pub home: String,
    /// `java -version` 解析结果；None = 无法解析（仍列出，供展示）
    pub version: Option<(u32, u32, u32)>,
}

impl JavaInstall {
    pub fn major(&self) -> u32 {
        self.version.map(|(m, _, _)| m).unwrap_or(0)
    }
    pub fn version_label(&self) -> String {
        match self.version {
            Some((a, b, c)) => format!("{a}.{b}.{c}"),
            None => "版本未知".into(),
        }
    }
}

/// 探测某个 home 的 java 版本（`<home>/bin/java -version`，输出在 stderr）。
fn probe_home_version(home: &str) -> Option<(u32, u32, u32)> {
    let exe = if cfg!(target_os = "windows") {
        format!("{home}\\bin\\java.exe")
    } else {
        format!("{home}/bin/java")
    };
    let out = std::process::Command::new(exe).arg("-version").output().ok()?;
    let text = if out.stderr.is_empty() {
        String::from_utf8_lossy(&out.stdout).to_string()
    } else {
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    crate::ops::environment::java_version_tuple(&text)
}

/// 解析 `/usr/libexec/java_home -V` 输出（stderr），提取每行末尾的 Home 路径。
/// 行样例：`    17.0.15 (arm64) "Homebrew" - "OpenJDK 17.0.15" /opt/homebrew/.../Home`
fn parse_java_home_v(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            // 有效行含 "(arch)" 标记与末尾路径；"Matching Java Virtual Machines" 表头被过滤
            if !l.contains('(') {
                return None;
            }
            l.split_whitespace().last().map(|s| s.to_string())
        })
        .filter(|p| p.starts_with('/'))
        .collect()
}

/// 列出本机全部 Java 运行时（去重、逐个探测版本；探测失败的目录剔除）。
#[cfg(target_os = "windows")]
pub fn installed_jdks() -> Vec<JavaInstall> {
    use std::collections::BTreeSet;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let mut homes: BTreeSet<String> = BTreeSet::new();
    // 注册表约定键：Oracle/Adoptium/Microsoft 安装器都会写 JavaHome（或 Path）指向 home 目录
    for sub in [
        r"SOFTWARE\JavaSoft\Java Runtime Environment",
        r"SOFTWARE\JavaSoft\JDK",
        r"SOFTWARE\JavaSoft\JRE",
        r"SOFTWARE\Eclipse Adoptium",
    ] {
        let Ok(root) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(sub) else {
            continue;
        };
        for name in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey(&name) else { continue };
            for val in ["JavaHome", "Path"] {
                if let Ok(p) = k.get_value::<String, _>(val) {
                    if !p.is_empty() {
                        homes.insert(p);
                    }
                }
            }
        }
    }
    // 目录兜底：MSI 默认安装位置（覆盖注册表键写法差异/权限受限场景）
    for dir in [
        r"C:\Program Files\Eclipse Adoptium",
        r"C:\Program Files\Java",
        r"C:\Program Files\Microsoft",
    ] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                if e.path().is_dir() {
                    homes.insert(e.path().to_string_lossy().to_string());
                }
            }
        }
    }
    homes
        .into_iter()
        .filter_map(|home| {
            let version = probe_home_version(&home)?;
            Some(JavaInstall {
                home,
                version: Some(version),
            })
        })
        .collect()
}

/// macOS（dev 机参考）：`/usr/libexec/java_home -V` 列出全部 JVM。
#[cfg(target_os = "macos")]
pub fn installed_jdks() -> Vec<JavaInstall> {
    let Ok(out) = std::process::Command::new("/usr/libexec/java_home").arg("-V").output() else {
        return vec![];
    };
    let text = String::from_utf8_lossy(&out.stderr);
    parse_java_home_v(&text)
        .into_iter()
        .filter_map(|home| {
            let version = probe_home_version(&home)?;
            Some(JavaInstall {
                home,
                version: Some(version),
            })
        })
        .collect()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn installed_jdks() -> Vec<JavaInstall> {
    vec![]
}

/// 在已装清单里找版本达标（>= expected）的 JDK，取最高版本。
pub fn find_compatible(expected_major: u32) -> Option<JavaInstall> {
    installed_jdks()
        .into_iter()
        .filter(|j| j.major() >= expected_major)
        .max_by_key(|j| j.version)
}

/// 修复任务状态（全局单例；前端轮询 GET /api/env/repair/status）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct RepairJob {
    pub running: bool,
    /// download → install → verify → done
    pub stage: String,
    /// 修复目标软件（java/mysql/redis/vernemq/…；None = 旧接口未标注）
    pub target: Option<String>,
    pub logs: Vec<String>,
    pub ok: Option<bool>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct RepairState {
    job: Mutex<RepairJob>,
}

impl RepairState {
    pub fn snapshot(&self) -> RepairJob {
        self.job.lock().unwrap().clone()
    }
    fn patch(&self, f: impl FnOnce(&mut RepairJob)) {
        let mut job = self.job.lock().unwrap();
        f(&mut job);
        if job.logs.len() > 200 {
            let drain = job.logs.len() - 200;
            job.logs.drain(..drain);
        }
    }
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    fn log(&self, msg: impl Into<String>) {
        let msg = msg.into();
        self.patch(move |j| j.logs.push(msg));
    }
}

/// 环境检查全量缓存类型（修复完成后清空，让下一次 /api/env 重新实测而非用 60s 旧缓存）。
pub type EnvCache = Arc<Mutex<Option<(std::time::Instant, Vec<crate::ops::environment::ServiceEnvReport>)>>>;

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const ADOPTIUM_INSTALLER_URL: &str =
    "https://api.adoptium.net/v3/installer/latest/{major}/ga/windows/x64/jre/hotspot/normal/eclipse";

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

/// Windows 安装流水线：下载 MSI → UAC 提权静默安装 → 复探验证。
#[cfg(target_os = "windows")]
async fn run_install(state: Arc<RepairState>, expected_major: u32, env_cache: EnvCache) {
    state.log(format!(
        "开始修复：目标 JDK {expected_major}+（Eclipse Temurin JRE，Adoptium 官方源）"
    ));

    // ===== 1. 下载 MSI =====
    state.patch(|j| j.stage = "download".into());
    let url = ADOPTIUM_INSTALLER_URL.replace("{major}", &expected_major.to_string());
    let msi = std::env::temp_dir().join(format!("dfy-ops-temurin{expected_major}-jre.msi"));
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
        std::fs::write(&msi, &bytes).map_err(|e| format!("写入临时文件失败：{e}"))?;
        Ok(bytes.len() as u64)
    };
    match dl.await {
        Ok(size) => state.log(format!("安装包下载完成：{}（{} MB）", msi.display(), size / 1024 / 1024)),
        Err(e) => {
            finish(state, env_cache, false, e).await;
            return;
        }
    }

    // ===== 2. UAC 提权静默安装（用户在 UAC 弹窗确认 = 授权动作） =====
    state.patch(|j| {
        j.stage = "install".into();
        j.logs.push(
            "正在启动 MSI 静默安装（FeatureJavaHome + FeatureEnvironment 会自动配置 JAVA_HOME 与 PATH）…".into(),
        );
        j.logs.push("请在弹出的 UAC 管理员确认框中点击「是」…".into());
    });
    let state_cl = state.clone();
    let install = tokio::task::spawn_blocking(move || {
        let ps = format!(
            "Start-Process msiexec -ArgumentList '/i \"{}\" /quiet ADDLOCAL=FeatureMain,FeatureEnvironment,FeatureJarFileRunWith,FeatureJavaHome' -Verb RunAs -Wait",
            msi.display()
        );
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
    })
    .await;
    match install {
        Ok(Ok(out)) if out.status.success() => {
            // 注意：此处形参是 impl Into<String>，Windows rustc 对字面量 .into() 会报
            // E0283 推断歧义（CI 实测），直接传 &str 即可，不要加 .into()。
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

    // ===== 3. 复探验证（新装 PATH 对本进程不可见，必须走注册表/目录扫描 + 绝对路径探测） =====
    state.patch(|j| j.stage = "verify".into());
    let found = tokio::task::spawn_blocking(move || find_compatible(expected_major))
        .await
        .ok()
        .flatten();
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

fn mysql_ini_path(home: &str) -> String {
    format!("{home}\\my.ini")
}

/// my.ini 内容：路径统一用正斜杠（MySQL 兼容），datadir 指向 home/data。
fn build_mysql_ini(home: &str) -> String {
    let h = home.replace('\\', "/");
    format!(
        "[mysqld]\nbasedir={h}\ndatadir={h}/data\nport=3306\ncharacter-set-server=utf8mb4\ncollation-server=utf8mb4_unicode_ci\ndefault-storage-engine=INNODB\n"
    )
}

/// Redis 配置路径：zip 自带 redis.windows.conf 则用之，否则用平台最小配置。
fn redis_conf_path(home: &str) -> String {
    let bundled = format!("{home}\\redis.windows.conf");
    if std::path::Path::new(&bundled).is_file() {
        bundled
    } else {
        format!("{home}\\redis-dfy.conf")
    }
}

/// Redis 最小配置（zip 不带 redis.windows.conf 时的兜底）。
fn build_redis_conf() -> String {
    "port 6379\nbind 0.0.0.0\n".to_string()
}

/// 通用修复任务：ZIP 免安装版离线安装（mysql/redis 的可靠离线路径——
/// MySQL 官方 msi 多为 Installer 束，静默执行装不上 Server，zip 才能完全离线装好）。
/// 流水线：解压定位 → 写配置+初始化数据 → 注册并启动 Windows 服务（一次 UAC）→ 端口验证。
/// install_dir 取服务注册表该服务的 installDir（如 C:/services/mysql）。
#[cfg(target_os = "windows")]
pub fn start_zip_install(
    target: &'static str,
    zip_path: String,
    install_dir: String,
    env_cache: EnvCache,
    state: Arc<RepairState>,
) -> Result<(), String> {
    if service_spec(target).is_none() {
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
#[cfg(target_os = "windows")]
fn runas_wait(exe: &str, args: &str) -> Result<(), String> {
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
            let mysqld = format!("{home}\\bin\\mysqld.exe");
            let out = std::process::Command::new(&mysqld)
                // --defaults-file 必须是首个选项（mysqld 的解析要求）
                .args([format!("--defaults-file={ini}"), "--initialize-insecure", "--console"])
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
            "net stop {svc} & sc delete {svc} & \"{}\" --install {svc} --defaults-file=\"{}\" & net start {svc}",
            format!("{home}\\bin\\mysqld.exe"),
            mysql_ini_path(home)
        ),
        "redis" => format!(
            "net stop {svc} & sc delete {svc} & \"{}\" --service-install \"{}\" --service-name {svc} & net start {svc}",
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
    let (svc, port) = service_spec(target).unwrap();
    state.log(format!("开始安装 {label}（免安装版）：{zip_path} → {install_dir}"));

    // ===== 解压 + 定位主目录 =====
    state.patch(|j| j.stage = "extract".into());
    state.log("正在解压安装包（大包需 1-2 分钟）…");
    let marker = if target == "mysql" { r"bin\mysqld.exe" } else { "redis-server.exe" };
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

    // ===== 注册 + 启动服务（一次 UAC） =====
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

    // ===== 端口验证 =====
    state.patch(|j| j.stage = "verify".into());
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

// ===== 卸载 / 维护（mysql/redis：停服务 → 注销服务 → 可选删目录） =====

/// Windows 服务是否存在（sc query 退出码 0 = 存在；探测无需管理员权限）。
pub fn service_exists(svc: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("sc")
            .args(["query", svc])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = svc;
        false
    }
}

/// 卸载任务（mysql/redis 维护）：停止并注销 Windows 服务，可选删除安装目录
/// （含数据——必须由用户在弹窗里显式勾选，数据库数据删除后不可恢复）。
/// install_dir 与安装管线同源（服务注册表 installDir），保证装、卸口径一致。
/// 成败以执行后的重探为准（服务不存在时 net stop/sc delete 的报错与退出码不作数）。
#[cfg(target_os = "windows")]
pub fn start_uninstall(
    target: &'static str,
    remove_data: bool,
    install_dir: String,
    env_cache: EnvCache,
    state: Arc<RepairState>,
) -> Result<(), String> {
    if service_spec(target).is_none() {
        return Err(format!(
            "{} 不支持平台内卸载（当前覆盖 MySQL/Redis），请使用 AI 方案或控制面板手动卸载",
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
        job.stage = "uninstall".into();
        job.ok = None;
        job.error = None;
        job.logs.clear();
    }
    tokio::spawn(async move {
        run_uninstall(state, target, remove_data, install_dir, env_cache).await;
    });
    Ok(())
}

/// 非 Windows：卸载管线仅在 Windows 目标机可用。
#[cfg(not(target_os = "windows"))]
pub fn start_uninstall(
    target: &'static str,
    _remove_data: bool,
    _install_dir: String,
    _env_cache: EnvCache,
    _state: Arc<RepairState>,
) -> Result<(), String> {
    Err(format!(
        "平台内卸载仅支持 Windows 目标机，{} 请手动卸载",
        crate::ops::resource::target_label(target)
    ))
}

#[cfg(target_os = "windows")]
async fn run_uninstall(
    state: Arc<RepairState>,
    target: &'static str,
    remove_data: bool,
    install_dir: String,
    env_cache: EnvCache,
) {
    let label = crate::ops::resource::target_label(target);
    let (svc, _port) = service_spec(target).unwrap();
    state.log(format!("开始卸载 {label}（服务 {svc}，目录 {install_dir}）"));

    // 前置探测：无服务且无目录 = 没装过，不用折腾 UAC
    let svc_installed = {
        let s = svc;
        tokio::task::spawn_blocking(move || service_exists(s)).await.unwrap_or(false)
    };
    let dir_exists = std::path::Path::new(&install_dir).is_dir();
    if !svc_installed && !dir_exists {
        finish(
            state,
            env_cache,
            false,
            format!("未检测到 {label} 的安装痕迹（无 Windows 服务 {svc}，目录 {install_dir} 也不存在）"),
        )
        .await;
        return;
    }
    if svc_installed {
        state.log(format!("检测到 Windows 服务 {svc}，将停止并注销… 请在 UAC 弹窗中点击「是」…"));
    }

    state.patch(|j| j.stage = "uninstall".into());
    // 一次 UAC：停服务 + 注销服务 (+ 删目录)。服务不存在时前两条报错无碍，
    // 退出码也不作数——成败以执行后的重探为准
    let script = if remove_data {
        format!("net stop {svc} & sc delete {svc} & rmdir /s /q \"{install_dir}\"")
    } else {
        format!("net stop {svc} & sc delete {svc}")
    };
    let run = tokio::task::spawn_blocking(move || runas_wait("cmd.exe", &format!("/c {script}"))).await;
    match run {
        Ok(Ok(())) => {}
        Ok(Err(e)) => state.log(format!("清理命令返回：{e}（将以实际状态复核）")),
        Err(e) => {
            finish(state, env_cache, false, format!("卸载任务异常：{e}")).await;
            return;
        }
    }

    // ===== 复核 =====
    state.patch(|j| j.stage = "verify".into());
    let svc_gone = {
        let s = svc;
        tokio::task::spawn_blocking(move || service_exists(s)).await.unwrap_or(true)
    };
    let dir_gone = !std::path::Path::new(&install_dir).exists();
    if svc_gone && (!remove_data || dir_gone) {
        state.log("卸载完成");
        if !remove_data {
            state.log(format!("已保留程序与数据目录 {install_dir}（重装可复用数据）"));
        }
        state.log("请点击「重新检查」刷新环境状态");
        finish(state, env_cache, true, String::new()).await;
    } else {
        finish(
            state,
            env_cache,
            false,
            format!(
                "卸载未完成：服务 {svc} {}，目录 {}（可能 UAC 被取消）——请重试或到 Windows 服务管理器手动处理",
                if svc_gone { "已注销" } else { "仍存在" },
                if dir_gone { "已删除" } else { "仍保留" }
            ),
        )
        .await;
    }
}

/// 收尾：更新任务终态 + 清空环境检查缓存（下一次 /api/env 全量实测）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
async fn finish(state: Arc<RepairState>, env_cache: EnvCache, ok: bool, error: String) {
    if !error.is_empty() {
        state.log(format!("失败：{error}"));
    }
    state.patch(|j| {
        j.running = false;
        j.stage = "done".into();
        j.ok = Some(ok);
        j.error = if error.is_empty() { None } else { Some(error) };
    });
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(mut guard) = env_cache.lock() {
            *guard = None;
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// java_home -V 输出解析：提取 4 个 Home 路径，过滤表头行。
    #[test]
    fn parse_java_home_v_extracts_paths() {
        let sample = "Matching Java Virtual Machines (4):\n\
             \x20   21.0.8 (arm64) \"JetBrains s.r.o.\" - \"JBR-21.0.8+9-1163.62-nomod 21.0.8\" /Users/x/Library/Java/JavaVirtualMachines/jbr-21.0.8/Contents/Home\n\
             \x20   17.0.15 (arm64) \"Homebrew\" - \"OpenJDK 17.0.15\" /opt/homebrew/Cellar/openjdk@17/17.0.15/libexec/openjdk.jdk/Contents/Home\n\
             \x20   11.0.29 (arm64) \"Microsoft\" - \"OpenJDK 11.0.29\" /Users/x/Library/Java/JavaVirtualMachines/ms-11.0.29/Contents/Home\n\
             \x20\n\
             \x20/Users/x/Library/Java/JavaVirtualMachines/jbr-21.0.8/Contents/Home\n";
        let paths = parse_java_home_v(sample);
        // 3 个 JVM 行各提取一个路径；末尾「默认 Home」行与表头行（无 (arch) 标记）被过滤——
        // 该路径与 JVM 行重复，少提取无碍（消费端按 home 去重探测）
        assert_eq!(paths.len(), 3, "应提取 3 个 JVM Home 路径: {paths:?}");
        assert!(paths[0].ends_with("jbr-21.0.8/Contents/Home"));
        assert!(paths[1].contains("openjdk@17"));
    }

    /// zip 免安装版管线：服务名/端口映射与配置生成。
    #[test]
    fn zip_pipeline_spec_and_configs() {
        assert_eq!(service_spec("mysql"), Some(("MySQL80", 3306)));
        assert_eq!(service_spec("redis"), Some(("Redis", 6379)));
        assert_eq!(service_spec("java"), None, "java 走 msi/在线，无 zip 管线");

        // my.ini：路径转正斜杠，datadir 固定 home/data，端口 3306
        let ini = build_mysql_ini(r"C:\services\mysql");
        assert!(ini.contains("basedir=C:/services/mysql"));
        assert!(ini.contains("datadir=C:/services/mysql/data"));
        assert!(ini.contains("port=3306"));
        assert!(!ini.contains('\\'), "my.ini 路径不应含反斜杠: {ini}");

        // redis 兜底最小配置
        assert_eq!(build_redis_conf(), "port 6379\nbind 0.0.0.0\n");
    }
}
