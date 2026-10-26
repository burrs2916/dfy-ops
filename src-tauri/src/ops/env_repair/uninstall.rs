//! 卸载管线：停止服务 → 注销服务 → 备份数据 → 卸载（MSI 静默 / 免安装目录清理）。

use std::sync::Arc;

use super::{EnvCache, RepairState};
#[cfg(target_os = "windows")]
use super::{finish, JDK_MANAGED_HOME, managed_java_home, remove_user_path_entry, runas_wait, service_spec};


// ===== 卸载 / 维护（mysql/redis：停服务 → 注销服务 → 可选删目录；java：MSI 静默卸载） =====

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

/// 服务是否处于 RUNNING（sc query 的 STATE 行；探测无需管理员权限）。
pub fn service_running(svc: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("sc")
            .args(["query", svc])
            .output()
            .map(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .lines()
                    .any(|l| l.trim_start().starts_with("STATE") && l.contains("RUNNING"))
            })
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = svc;
        false
    }
}

/// DisplayName 是否像 Java/JDK MSI 产品（避免 "JavaScript ..." 之类误伤）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn java_product_name(display: &str) -> bool {
    let d = display.to_lowercase();
    d.contains("temurin")
        || d.contains("adoptium")
        || d.contains("openjdk")
        || d.contains("jdk")
        || d.contains("jre")
        || (d.contains("java") && (d.contains("se ") || d.contains("update") || d.contains("development")))
}

/// DisplayName 是否为 MySQL MSI 产品（MySQL Installer 装的 Server/Connector 等）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn mysql_product_name(display: &str) -> bool {
    display.to_lowercase().contains("mysql")
}

/// 从 MSI 形态的 UninstallString 提取产品码 {GUID}；非 msiexec 条目返回 None。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn extract_msi_guid(uninstall_string: &str) -> Option<String> {
    let s = uninstall_string.trim();
    if !s.to_lowercase().contains("msiexec") {
        return None;
    }
    let start = s.find('{')?;
    let end = s.find('}')?;
    (end > start).then(|| s[start..=end].to_string())
}

/// 按名称过滤枚举注册表卸载条目里的 MSI 产品（DisplayName + 产品码 {GUID}）。
/// java/mysql 的「完全清理」共用：对机器上已有的外部 MSI 安装也能静默卸载干净。
#[cfg(target_os = "windows")]
fn msi_uninstall_entries(pred: fn(&str) -> bool) -> Vec<(String, String)> {
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;
    let mut out: Vec<(String, String)> = Vec::new();
    for sub in [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ] {
        let Ok(root) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(sub) else {
            continue;
        };
        for name in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey(&name) else { continue };
            let dn = k.get_value::<String, _>("DisplayName").unwrap_or_default();
            if dn.is_empty() || !pred(&dn) {
                continue;
            }
            let us = k.get_value::<String, _>("UninstallString").unwrap_or_default();
            if let Some(guid) = extract_msi_guid(&us) {
                if !out.iter().any(|(_, g)| g == &guid) {
                    out.push((dn, guid));
                }
            }
        }
    }
    out
}

/// 注册表卸载条目里的 Java MSI 产品（Temurin/Adoptium/Oracle/Microsoft OpenJDK 等）；
/// zip 免安装版没有卸载条目，自然不在列表（那类卸载 = 删平台目录 + 清 JAVA_HOME）。
#[cfg(target_os = "windows")]
pub fn java_msi_entries() -> Vec<(String, String)> {
    msi_uninstall_entries(java_product_name)
}

/// MSI 安装的 java 版本标签（卡片展示用）：从第一条 DisplayName 提取首个
/// 纯数字词，如 "Eclipse Temurin JRE 21 (x64)" → "21"、"Java 8 Update 301"
/// → "8"。无条目或无法解析返回 None。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn java_msi_version_label() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let entries = java_msi_entries();
        let dn = entries.first()?.0.clone();
        dn.split(|c: char| !c.is_ascii_digit())
            .find(|p| !p.is_empty())
            .map(String::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// 注册表卸载条目里的 MySQL MSI 产品（完全清理用：平台卸载时一并静默卸载）。
#[cfg(target_os = "windows")]
pub fn mysql_msi_entries() -> Vec<(String, String)> {
    msi_uninstall_entries(mysql_product_name)
}

/// 非 Windows：无 MSI 注册表，恒为空（卸载入口在 start_uninstall 已按平台挡住）。
#[cfg(not(target_os = "windows"))]
pub fn java_msi_entries() -> Vec<(String, String)> {
    Vec::new()
}

/// 非 Windows：恒为空。
#[cfg(not(target_os = "windows"))]
pub fn mysql_msi_entries() -> Vec<(String, String)> {
    Vec::new()
}

/// 卸载任务（维护）：mysql/redis = 停止并注销 Windows 服务，可选删除安装目录
/// （含数据——必须由用户在弹窗里显式勾选，数据库数据删除后不可恢复）；
/// java = 枚举注册表 MSI 条目后 msiexec /x 静默卸载（remove_data 不适用）。
/// mysql 勾选删数据时，删除前先自动备份全部非系统数据库为单个 SQL 文件
/// （表结构+数据+例程，落 <config_dir>/backups；备份失败中止删除，数据分毫不动）。
/// install_dir 与安装管线同源（服务注册表 installDir），保证装、卸口径一致。
/// 成败以执行后的重探为准（服务不存在时 net stop/sc delete 的报错与退出码不作数）。
#[cfg(target_os = "windows")]
pub fn start_uninstall(
    target: &'static str,
    remove_data: bool,
    install_dir: String,
    config_dir: String,
    env_cache: EnvCache,
    state: Arc<RepairState>,
) -> Result<(), String> {
    if !matches!(target, "mysql" | "redis" | "java") {
        return Err(format!(
            "{} 不支持平台内卸载（当前覆盖 MySQL/Redis/Java），请使用 AI 方案或控制面板手动卸载",
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
        job.op = "uninstall".into();
        job.stage = "uninstall".into();
        job.ok = None;
        job.error = None;
        job.logs.clear();
    }
    tokio::spawn(async move {
        run_uninstall(state, target, remove_data, install_dir, config_dir, env_cache).await;
    });
    Ok(())
}

/// 非 Windows：卸载管线仅在 Windows 目标机可用。
#[cfg(not(target_os = "windows"))]
pub fn start_uninstall(
    target: &'static str,
    _remove_data: bool,
    _install_dir: String,
    _config_dir: String,
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
    config_dir: String,
    env_cache: EnvCache,
) {
    if target == "java" {
        run_java_uninstall(state, env_cache).await;
        return;
    }
    let label = crate::ops::resource::target_label(target);
    let (svc, _port) = service_spec(target).unwrap();
    state.log(format!("开始卸载 {label}（服务 {svc}，目录 {install_dir}）"));

    // ===== 数据安全闸门：mysql 勾选删数据时，先备份全部非系统数据库为 SQL 文件 =====
    // 备份失败 = 数据可能无法恢复 → 直接中止删除（数据分毫不动），绝无侥幸
    if target == "mysql" && remove_data {
        state.patch(|j| j.stage = "backup".into());
        state.log("即将删除数据目录——正在先把全部非系统数据库（表结构 + 数据 + 例程）备份为 SQL 文件（mysql/sys 等系统库跳过）…");
        let install_dir_for_backup = install_dir.clone();
        let config_dir_for_backup = config_dir.clone();
        let b = tokio::task::spawn_blocking(move || {
            backup_mysql_before_delete(&install_dir_for_backup, &config_dir_for_backup)
        })
        .await;
        match b {
            Ok(Ok(Some(path))) => {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                state.log(format!(
                    "备份完成：{path}（{} KB）。删除后可在「数据库运维 → 服务器 → 备份与恢复」用该文件一键恢复业务数据",
                    size / 1024
                ));
            }
            Ok(Ok(None)) => {
                state.log("实例中没有非系统数据库，无需备份，继续删除");
            }
            Ok(Err(e)) => {
                finish(
                    state,
                    env_cache,
                    false,
                    format!("数据备份失败，已中止删除（数据未受影响）：{e}"),
                )
                .await;
                return;
            }
            Err(e) => {
                finish(state, env_cache, false, format!("备份任务异常，已中止删除：{e}")).await;
                return;
            }
        }
    }

    // MySQL「完全清理」：机器上可能有外部 MSI 安装（MySQL Installer 装的 Server），
    // 一并枚举后静默卸载，不留注册表产品残留
    let msi_entries = if target == "mysql" {
        tokio::task::spawn_blocking(mysql_msi_entries).await.unwrap_or_default()
    } else {
        Vec::new()
    };
    if !msi_entries.is_empty() {
        state.log(format!(
            "检测到 {} 个 MySQL MSI 产品条目，将一并静默卸载…",
            msi_entries.len()
        ));
    }

    // 前置探测：无服务、无目录、无 MSI 条目 = 没装过，不用折腾 UAC
    let svc_installed = {
        let s = svc;
        tokio::task::spawn_blocking(move || service_exists(s)).await.unwrap_or(false)
    };
    let dir_exists = std::path::Path::new(&install_dir).is_dir();
    if !svc_installed && !dir_exists && msi_entries.is_empty() {
        finish(
            state,
            env_cache,
            false,
            format!("未检测到 {label} 的安装痕迹（无 Windows 服务 {svc}，目录 {install_dir} 也不存在，无 MSI 产品条目）"),
        )
        .await;
        return;
    }
    if svc_installed {
        state.log(format!("检测到 Windows 服务 {svc}，将停止并注销… 请在 UAC 弹窗中点击「是」…"));
    }

    state.patch(|j| j.stage = "uninstall".into());
    // 一次 UAC：MSI 卸载 + 停服务 + 注销服务 (+ 删目录)。各段在对应对象不存在时
    // 报错无碍，退出码也不作数——成败以执行后的重探为准
    let mut parts: Vec<String> = msi_entries
        .iter()
        .map(|(_, g)| format!("msiexec /x {g} /quiet /norestart"))
        .collect();
    parts.push(format!("net stop {svc}"));
    // 进程未退出（InnoDB 落盘慢、服务 STOP_PENDING）就 sc delete 会得到
    // 「服务已标记为删除」而失败，目录也会因文件被占用删不掉——优雅停服后
    // 强杀残留进程，等句柄释放再注销
    let proc_image = if target == "mysql" { "mysqld.exe" } else { "redis-server.exe" };
    parts.push(format!("taskkill /f /im {proc_image}"));
    // ping 当 sleep：timeout 命令在部分非交互上下文会拒绝等待
    parts.push("ping -n 3 127.0.0.1 >nul".to_string());
    parts.push(format!("sc delete {svc}"));
    if remove_data {
        parts.push(format!("rmdir /s /q \"{install_dir}\""));
        // MSI 版的数据/配置目录（MySQL Installer 默认落在 ProgramData）
        parts.push(r#"rmdir /s /q "C:\ProgramData\MySQL""#.to_string());
    }
    let script = parts.join(" & ");
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
    // sc delete 是「标记删除」，句柄全释放才真正消失——轮询几次再下结论
    let svc_gone = {
        let s = svc;
        tokio::task::spawn_blocking(move || {
            for _ in 0..5 {
                if !service_exists(s) {
                    return true;
                }
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
            false
        })
        .await
        .unwrap_or(true)
    };
    let dir_gone = !std::path::Path::new(&install_dir).exists();
    let msi_gone = if target == "mysql" {
        tokio::task::spawn_blocking(|| mysql_msi_entries().is_empty()).await.unwrap_or(true)
    } else {
        true
    };
    if svc_gone && msi_gone && (!remove_data || dir_gone) {
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
                "卸载未完成：服务 {svc} {}，MSI 条目 {}，目录 {}（可能 UAC 被取消）——请重试或到 Windows 服务管理器手动处理",
                if svc_gone { "已注销" } else { "仍存在" },
                if msi_gone { "已清除" } else { "仍有残留" },
                if dir_gone { "已删除" } else { "仍保留" }
            ),
        )
        .await;
    }
}

/// Java 卸载（完全清理）：MSI 条目静默卸载 + 平台免安装目录删除 + 用户级
/// JAVA_HOME 清除，一次 UAC 内完成，复核三项全清。
#[cfg(target_os = "windows")]
async fn run_java_uninstall(state: Arc<RepairState>, env_cache: EnvCache) {
    state.log("正在枚举本机 MSI 方式安装的 Java/JDK 条目…");
    let entries = tokio::task::spawn_blocking(java_msi_entries).await.unwrap_or_default();
    // 与安装/探测同口径：覆盖嵌套布局（Temurin zip 解压后 bin 不在顶层），
    // 只查顶层 bin 会把嵌套安装误判成「无安装痕迹」而拒绝卸载
    let managed = managed_java_home().is_some();
    if entries.is_empty() && !managed {
        finish(
            state,
            env_cache,
            false,
            format!("未检测到 Java 安装痕迹（无 MSI 卸载条目，平台免安装目录 {JDK_MANAGED_HOME} 也不存在）"),
        )
        .await;
        return;
    }
    state.patch(|j| j.stage = "uninstall".into());
    for (dn, _) in &entries {
        state.log(format!("待卸载（MSI）：{dn}"));
    }
    if managed {
        state.log(format!("待删除（免安装）：平台 JDK 目录 {JDK_MANAGED_HOME}"));
    }
    state.log("将清除用户级 JAVA_HOME 并从用户 PATH 移除平台 JDK… 请在 UAC 弹窗中点击「是」…");
    // 记录卸载前的托管主目录（删除后 managed_java_home 探测不到，PATH 清理要用）
    let managed_home_before = if managed { managed_java_home() } else { None };
    // 一次 UAC 内完成全部清理：MSI 静默逐条卸载 + 删平台目录 + 删用户级 JAVA_HOME
    // （reg delete 在变量不存在时报错，无碍；成败以复核为准）
    let mut parts: Vec<String> = entries
        .iter()
        .map(|(_, g)| format!("msiexec /x {g} /quiet /norestart"))
        .collect();
    parts.push(format!("rmdir /s /q \"{JDK_MANAGED_HOME}\""));
    parts.push("reg delete HKCU\\Environment /v JAVA_HOME /f".to_string());
    let script = parts.join(" & ");
    let run = tokio::task::spawn_blocking(move || runas_wait("cmd.exe", &format!("/c {script}"))).await;
    match run {
        Ok(Ok(())) => {}
        Ok(Err(e)) => state.log(format!("清理命令返回：{e}（将以实际状态复核）")),
        Err(e) => {
            finish(state, env_cache, false, format!("卸载任务异常：{e}")).await;
            return;
        }
    }
    // 用户 PATH 里的 {home}\bin 条目：注册表操作无需提权，在 Rust 侧清理
    if let Some(home) = managed_home_before {
        let bin = format!("{home}\\bin");
        match tokio::task::spawn_blocking(move || remove_user_path_entry(&bin)).await {
            Ok(Ok(true)) => state.log("已从用户 PATH 移除平台 JDK bin 条目"),
            Ok(Ok(false)) => {}
            Ok(Err(e)) => state.log(format!("PATH 条目清理失败：{e}")),
            Err(e) => state.log(format!("PATH 条目清理任务异常：{e}")),
        }
    }

    // ===== 复核 =====
    state.patch(|j| j.stage = "verify".into());
    let after = tokio::task::spawn_blocking(java_msi_entries).await.unwrap_or_default();
    let dir_gone = managed_java_home().is_none();
    if after.is_empty() && dir_gone {
        state.log("卸载完成：MSI 条目已清空，平台免安装目录已删除，JAVA_HOME 与 PATH 条目已清理");
        state.log("提示：已开终端里的 JAVA_HOME/PATH 是旧值，重开终端或重新登录后生效");
        finish(state, env_cache, true, String::new()).await;
    } else {
        finish(
            state,
            env_cache,
            false,
            format!(
                "卸载未完成：MSI 条目 {}，平台目录 {}（可能 UAC 被取消或 MSI 卸载被拒）——可到「设置 → 应用」手动处理",
                if after.is_empty() { "已清空" } else { "仍有残留" },
                if dir_gone { "已删除" } else { "仍保留" }
            ),
        )
        .await;
    }
}

/// MySQL datadir 是否已初始化（data/mysql 系统库目录存在）。
/// 卸载时保留数据 → 重装复用时必须跳过 --initialize-insecure（目录非空会失败）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(super) fn mysql_data_initialized(home: &str) -> bool {
    std::path::Path::new(home).join("data").join("mysql").is_dir()
}

/// UNIX 秒 → UTC "YYYYmmdd-HHMMSS"（civil_from_days 算法，不引时间库）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
/// Unix 秒 → "YYYYMMDD-HHMMSS"（UTC，文件名安全）。
pub(crate) fn timestamp_utc(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let (h, m, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}{mo:02}{d:02}-{h:02}{m:02}{s:02}")
}

/// 候选 mysqldump 路径（按优先级）：平台安装目录顶层 bin → 一层嵌套 bin（zip
/// 带顶层目录时主目录在嵌套层，与 locate_home 同口径）→ MySQL Installer 常见
/// 安装位 → PATH（平台无关字符串生成 + 嵌套实扫，供测试与查找共用）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
/// mysqldump 候选：平台目录顶层 bin → 嵌套子目录 → MSI 常见位 → PATH。
pub(crate) fn mysqldump_candidates(install_dir: &str) -> Vec<String> {
    let bin = |base: &std::path::Path| {
        base.join("bin").join("mysqldump.exe").to_string_lossy().to_string()
    };
    let root = std::path::Path::new(install_dir);
    let mut out = vec![bin(root)];
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.push(bin(&p));
            }
        }
    }
    for v in ["8.4", "8.0", "5.7"] {
        out.push(format!(r"C:\Program Files\MySQL\MySQL Server {v}\bin\mysqldump.exe"));
    }
    out.push("mysqldump.exe".into());
    out
}

/// 系统数据库清单：卸载前备份跳过这些（实例自带，无业务数据）。
#[cfg(target_os = "windows")]
const MYSQL_SYS_DBS: &[&str] = &["information_schema", "mysql", "performance_schema", "sys"];

/// 由 mysqldump 候选路径推导同目录 mysql.exe 客户端路径（枚举业务库用）。
#[cfg(target_os = "windows")]
fn mysql_client_from_dump(p: &str) -> String {
    if p.ends_with("mysqldump.exe") {
        p.replace("mysqldump.exe", "mysql.exe")
    } else if p.ends_with("mysqldump") {
        p.replace("mysqldump", "mysql")
    } else {
        p.to_string()
    }
}

/// 连接认证参数：优先平台托管凭据（config/mysql_credential.json），未托管时
/// 兜底 root 无密码。密码为空必须省略 --password，否则 CLI 会挂起等输入。
#[cfg(target_os = "windows")]
fn mysql_auth_args(port: u16, user: &str, password: &str) -> Vec<String> {
    let mut a = vec![
        "-h127.0.0.1".to_string(),
        format!("--port={port}"),
        format!("--user={user}"),
    ];
    if !password.is_empty() {
        a.push(format!("--password={password}"));
    }
    a
}

/// 卸载前备份（数据保险）：仅导出非系统数据库（表结构 + 数据 + 例程 + 触发器）
/// 为单个 SQL 文件，落 <工作目录>/backups。服务未运行时先经 UAC 启动。任何一步
/// 失败都返回 Err（调用方据此中止删除——宁可卸载不成，不可数据白丢）。
#[cfg(target_os = "windows")]
fn backup_mysql_before_delete(install_dir: &str, config_dir: &str) -> Result<Option<String>, String> {
    // 1. 定位 mysqldump 与同目录 mysql.exe 客户端
    let dump_exe = mysqldump_candidates(install_dir)
        .into_iter()
        .find(|p| {
            if p.contains('\\') {
                std::path::Path::new(p).is_file()
            } else {
                // PATH 兜底：能用 --version 跑通即视为可用
                std::process::Command::new(p)
                    .arg("--version")
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            }
        })
        .ok_or_else(|| {
            "未找到 mysqldump.exe（安装目录与常见 MySQL Server 路径均无）。请手动备份数据目录后再执行删除"
                .to_string()
        })?;
    let dump_client = mysqldump_candidates(install_dir)
        .into_iter()
        .find(|p| {
            let c = mysql_client_from_dump(p);
            if c.contains('\\') {
                std::path::Path::new(&c).is_file()
            } else {
                std::process::Command::new(&c).arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
            }
        })
        .map(|p| mysql_client_from_dump(&p))
        .ok_or_else(|| "未找到 mysql.exe 客户端，无法枚举数据库清单".to_string())?;

    // 2. 确保服务在运行（mysqldump 需要连接实例；未运行则 UAC 启动后等就绪）
    if !crate::ops::health::check_tcp("127.0.0.1", 3306, 800) {
        let (svc, _) = service_spec("mysql").unwrap();
        if !service_exists(&svc) {
            return Err(format!("MySQL 服务 {svc} 不存在且端口未监听，无实例可备份"));
        }
        runas_wait("cmd.exe", &format!("/c net start {svc}"))
            .map_err(|e| format!("服务未运行且启动失败（UAC 取消？）：{e}"))?;
        let mut up = false;
        for _ in 0..15 {
            if crate::ops::health::check_tcp("127.0.0.1", 3306, 800) {
                up = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        if !up {
            return Err("服务已尝试启动但 3306 未监听，无法连接备份".to_string());
        }
    }

    // 3. 枚举非系统数据库（SHOW DATABASES 减系统库）
    let cred = crate::ops::mysql_credential::load(std::path::Path::new(config_dir));
    let enum_out = std::process::Command::new(&dump_client)
        .args(mysql_auth_args(cred.port, &cred.user, &cred.password))
        .args(["-B", "-e", "SHOW DATABASES"])
        .output()
        .map_err(|e| format!("无法运行 mysql.exe 枚举数据库：{e}"))?;
    if !enum_out.status.success() {
        let err = String::from_utf8_lossy(&enum_out.stderr).trim().to_string();
        return Err(format!("枚举数据库失败（检查连接凭据）：{err}"));
    }
    let dbs: Vec<String> = String::from_utf8_lossy(&enum_out.stdout)
        .lines()
        .skip(1) // -B 批量模式首行是列名 Database
        .map(|l| l.trim().to_string())
        .filter(|d| !d.is_empty() && !MYSQL_SYS_DBS.contains(&d.to_ascii_lowercase().as_str()))
        .collect();
    if dbs.is_empty() {
        return Ok(None);
    }

    // 4. 按库导出：--databases 带建库语句；--single-transaction 一致性快照不锁库；
    //    --result-file 直写文件避免 shell 重定向的编码问题（UTF-8 原样落盘）
    let backup_dir = std::path::Path::new(config_dir).join("backups");
    std::fs::create_dir_all(&backup_dir).map_err(|e| format!("创建备份目录 {} 失败：{e}", backup_dir.display()))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    // 毫秒后缀：卸载备份是强确认才触发的大动作，但同秒并发两次仍会
    // 互相覆盖——备份文件一旦被覆盖就找不回来，毫秒级区分成本低得多
    let out_file = backup_dir.join(format!(
        "mysql-biz-{}.{:03}.sql",
        timestamp_utc(now.as_secs()),
        now.subsec_millis()
    ));
    let out = std::process::Command::new(&dump_exe)
        .args(mysql_auth_args(cred.port, &cred.user, &cred.password))
        .args([
            "--single-transaction",
            "--routines",
            "--triggers",
            "--events",
            "--databases",
        ])
        .args(&dbs)
        .arg(format!("--result-file={}", out_file.display()))
        .output()
        .map_err(|e| format!("无法运行 mysqldump：{e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let err = stderr.trim();
        let _ = std::fs::remove_file(&out_file);
        return Err(format!("mysqldump 导出失败：{err}"));
    }
    let size = std::fs::metadata(&out_file).map(|m| m.len()).unwrap_or(0);
    if size == 0 {
        let _ = std::fs::remove_file(&out_file);
        return Err("备份文件为空，视为备份失败".to_string());
    }
    Ok(Some(out_file.to_string_lossy().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Java MSI 卸载：DisplayName 过滤（防 "JavaScript" 误伤）与产品码提取。
    #[test]
    fn java_msi_entry_parsing() {
        assert!(java_product_name("Eclipse Temurin JRE 21 (x64)"));
        assert!(java_product_name("Java 8 Update 301"));
        assert!(java_product_name("Microsoft OpenJDK 17"));
        assert!(java_product_name("Java SE Development Kit 21"));
        assert!(!java_product_name("JavaScript Runtime"));
        assert!(!java_product_name("MySQL Server 8.0"));

        assert_eq!(
            extract_msi_guid(r"MsiExec.exe /I{A1B2C3D4-1111-2222-3333-444455556666}"),
            Some("{A1B2C3D4-1111-2222-3333-444455556666}".into())
        );
        assert_eq!(
            extract_msi_guid(r"MsiExec.exe /X{ABCDEF00-0000-0000-0000-000000000000}"),
            Some("{ABCDEF00-0000-0000-0000-000000000000}".into())
        );
        assert_eq!(extract_msi_guid(r#""C:\x\uninstall.exe" /silent"#), None);
        assert_eq!(extract_msi_guid("MsiExec.exe /I no-guid"), None);
    }

    /// MySQL 重装复用数据目录：datadir 已初始化（data/mysql 存在）→ 跳过 initialize。
    #[test]
    fn mysql_data_initialized_detects_system_schema() {
        let tmp = std::env::temp_dir().join(format!("dfy-mysql-init-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("data").join("mysql")).unwrap();
        assert!(mysql_data_initialized(tmp.to_str().unwrap()));
        let partial = std::env::temp_dir().join(format!("dfy-mysql-partial-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&partial);
        std::fs::create_dir_all(partial.join("data")).unwrap();
        assert!(!mysql_data_initialized(partial.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::remove_dir_all(&partial);
    }

    /// 备份链路：mysqldump 候选顺序 + 时间戳格式。
    #[test]
    fn mysql_backup_helpers() {
        let cands = mysqldump_candidates(r"C:\services\mysql");
        assert!(cands[0].starts_with(r"C:\services\mysql"));
        assert!(cands[0].ends_with("mysqldump.exe") && cands[0].contains("bin"));
        assert!(cands.iter().any(|p| p.contains("MySQL Server 8.0")));
        assert_eq!(cands.last().unwrap(), "mysqldump.exe", "PATH 兜底必须最后");

        assert_eq!(timestamp_utc(0), "19700101-000000");
        assert_eq!(timestamp_utc(1_700_000_000), "20231114-221320");
        assert_eq!(timestamp_utc(86399), "19700101-235959");
        assert_eq!(timestamp_utc(86400), "19700102-000000");
    }
}
