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
pub fn probe_home_version(home: &str) -> Option<(u32, u32, u32)> {
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
/// 仅 macOS 产线使用；非 macOS 平台编译排除，测试跨平台保留。
#[cfg(any(target_os = "macos", test))]
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

/// 平台管理目录中的 Java 主目录：与安装管线 locate_home 同口径（顶层 bin
/// 或一层嵌套，如 jdk-17.0.20+1\bin）。探测必须覆盖嵌套布局——Temurin 官方
/// zip 解压后 bin 不在顶层，只查顶层会「装完仍显示未安装」。非 Windows 恒 None。
pub fn managed_java_home() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let root = std::path::Path::new(JDK_MANAGED_HOME);
        if root.join(r"bin\java.exe").is_file() {
            return Some(JDK_MANAGED_HOME.to_string());
        }
        if let Ok(entries) = std::fs::read_dir(root) {
            for e in entries.flatten() {
                if e.path().is_dir() && e.path().join(r"bin\java.exe").is_file() {
                    return Some(e.path().to_string_lossy().to_string());
                }
            }
        }
        None
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// JAVA_HOME 是否已在系统里配置（卡片如实展示「已配置/未配置」的依据）：
/// 用户级 HKCU\Environment（平台 zip 管线 setx 写入处）或系统级 HKLM Session
/// Manager\Environment（Temurin MSI 的 FeatureJavaHome 写入处）任一存在即算。
/// 传 managed home 时进一步要求值与其一致（平台接管口径，路径比较忽略大小写）。
/// 非 Windows 恒 false。
pub fn java_home_configured(managed: Option<&str>) -> bool {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        use winreg::RegKey;
        let read = |root: winreg::RegKey, path: &str| -> Option<String> {
            root.open_subkey(path)
                .ok()?
                .get_value::<String, _>("JAVA_HOME")
                .ok()
        };
        let hkcu = read(RegKey::predef(HKEY_CURRENT_USER), "Environment");
        let hklm = read(
            RegKey::predef(HKEY_LOCAL_MACHINE),
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        );
        match managed {
            Some(d) => [hkcu, hklm]
                .into_iter()
                .flatten()
                .any(|v| v.trim().eq_ignore_ascii_case(d)),
            None => hkcu.is_some() || hklm.is_some(),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = managed;
        false
    }
}

// ===== 用户级环境变量注册表操作（HKCU\Environment） =====
//
// 为什么不用 setx 改 PATH：setx 对超过 1024 字符的值会静默截断，而用户 PATH
// 常年累积很长，一旦截断就是破坏性事故。因此 PATH 一律走注册表原始读写
// （保持 REG_EXPAND_SZ 原类型，%VAR% 引用原样保留），写完主动广播
// WM_SETTINGCHANGE 让 Explorer/新开终端立即拿到新环境。

#[cfg(target_os = "windows")]
fn utf16le_bytes(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(|w| w.to_le_bytes())
        .collect()
}

#[cfg(target_os = "windows")]
fn utf16le_to_string(b: &[u8]) -> String {
    let words: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    String::from_utf16_lossy(&words).trim_end_matches('\0').to_string()
}

/// 广播环境变量变更（新开终端/Explorer 无需重启即可读到）。
#[cfg(target_os = "windows")]
fn broadcast_env_change() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };
    let param: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result: usize = 0;
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            param.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            2000,
            &mut result,
        );
    }
}

/// 规范化 PATH 条目用于比较：去首尾空白、去结尾反斜杠、忽略大小写。
#[cfg(target_os = "windows")]
fn norm_path_entry(s: &str) -> String {
    s.trim().trim_end_matches('\\').to_ascii_lowercase()
}

/// 读取用户级 PATH 原始值（未展开 %VAR%）与注册表类型；不存在返回空串 + EXPAND_SZ。
#[cfg(target_os = "windows")]
fn read_user_path_raw() -> Result<(String, winreg::enums::RegType), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_QUERY_VALUE};
    use winreg::{RegKey, RegValue};
    let root = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_QUERY_VALUE)
        .map_err(|e| format!("无法打开注册表环境键：{e}"))?;
    match root.get_raw_value("Path") {
        Ok(RegValue { bytes, vtype }) => Ok((utf16le_to_string(&bytes), vtype)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Ok((String::new(), winreg::enums::RegType::REG_EXPAND_SZ))
        }
        Err(e) => Err(format!("读取用户 PATH 失败：{e}")),
    }
}

/// 写回用户级 PATH（保持原注册表类型）并广播。
#[cfg(target_os = "windows")]
fn write_user_path_raw(value: &str, vtype: winreg::enums::RegType) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE};
    use winreg::{RegKey, RegValue};
    let root = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_SET_VALUE)
        .map_err(|e| format!("无法打开注册表环境键：{e}"))?;
    root.set_raw_value(
        "Path",
        &RegValue {
            bytes: utf16le_bytes(value),
            vtype,
        },
    )
    .map_err(|e| format!("写入用户 PATH 失败：{e}"))?;
    broadcast_env_change();
    Ok(())
}

/// 用户级 PATH 是否已包含某条目（探测卡片「PATH 已配置」用）。非 Windows 恒 false。
pub fn user_path_has(entry: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        read_user_path_raw()
            .map(|(raw, _)| {
                raw.split(';')
                    .any(|p: &str| norm_path_entry(p) == norm_path_entry(entry))
            })
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = entry;
        false
    }
}

/// 向用户级 PATH 追加条目（已存在则不动，返回 false）。仅 Windows 有实现与调用。
#[cfg(target_os = "windows")]
pub fn ensure_user_path_entry(entry: &str) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let (raw, vtype) = read_user_path_raw()?;
        if raw
            .split(';')
            .any(|p| norm_path_entry(p) == norm_path_entry(entry))
        {
            return Ok(false);
        }
        let mut parts: Vec<&str> = raw
            .split(';')
            .filter(|p| !p.trim().is_empty())
            .collect();
        parts.push(entry);
        write_user_path_raw(&parts.join(";"), vtype)?;
        Ok(true)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = entry;
        Err("仅支持 Windows".into())
    }
}

/// 从用户级 PATH 移除条目（不存在返回 false；卸载 java 清理用）。仅 Windows 有实现与调用。
#[cfg(target_os = "windows")]
pub fn remove_user_path_entry(entry: &str) -> Result<bool, String> {
    #[cfg(target_os = "windows")]
    {
        let (raw, vtype) = read_user_path_raw()?;
        let target = norm_path_entry(entry);
        let all: Vec<&str> = raw
            .split(';')
            .filter(|p| !p.trim().is_empty())
            .collect();
        let kept: Vec<&str> = all
            .iter()
            .copied()
            .filter(|p| norm_path_entry(p) != target)
            .collect();
        if kept.len() == all.len() {
            return Ok(false);
        }
        write_user_path_raw(&kept.join(";"), vtype)?;
        Ok(true)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = entry;
        Err("仅支持 Windows".into())
    }
}

/// 写用户级 JAVA_HOME（REG_SZ）并广播。
#[cfg(target_os = "windows")]
fn set_user_java_home(home: &str) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_SET_VALUE, RegType};
    use winreg::{RegKey, RegValue};
    let root = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_SET_VALUE)
        .map_err(|e| format!("无法打开注册表环境键：{e}"))?;
    root.set_raw_value(
        "JAVA_HOME",
        &RegValue {
            bytes: utf16le_bytes(home),
            vtype: RegType::REG_SZ,
        },
    )
    .map_err(|e| format!("写入 JAVA_HOME 失败：{e}"))?;
    broadcast_env_change();
    Ok(())
}

/// 一键修复/接管 java 用户级环境变量（幂等）：JAVA_HOME 指向平台托管主目录 +
/// PATH 追加 {home}\bin，使 cmd 里 `java -version` 直接可用。
/// 返回（JAVA_HOME 是否重写，PATH 是否新增）。需在 blocking 上下文调用。
pub fn java_env_ensure() -> Result<(bool, bool), String> {
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let Some(home) = managed_java_home() else {
            return Err("未检测到平台托管的 JDK 目录，请先安装 JDK".into());
        };
        let current = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey("Environment")
            .ok()
            .and_then(|k| k.get_value::<String, _>("JAVA_HOME").ok());
        let matches = current
            .as_deref()
            .map(|v| v.trim().trim_end_matches('\\').eq_ignore_ascii_case(&home))
            .unwrap_or(false);
        let jh_written = if matches { false } else { set_user_java_home(&home)?; true };
        let bin = format!("{home}\\bin");
        let path_added = ensure_user_path_entry(&bin)?;
        Ok((jh_written, path_added))
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("仅支持 Windows".into())
    }
}

/// 列出本机全部 Java 运行时（去重、逐个探测版本；探测失败的目录剔除）。
#[cfg(target_os = "windows")]
pub fn installed_jdks() -> Vec<JavaInstall> {
    use std::collections::BTreeSet;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let mut homes: BTreeSet<String> = BTreeSet::new();
    // 平台免安装目录优先：我们的 zip 管线装的 JDK（无注册表产品，检测必须走目录）
    if let Some(home) = managed_java_home() {
        homes.insert(home);
    }
    // JAVA_HOME 兜底：zip 解压安装的 JDK 往往无注册表条目、不在常见目录、
    // 也不进 PATH（如 CI runner 的 hostedtoolcache），只设了 JAVA_HOME——
    // 环境变量是最后一道可靠的归属线索
    if let Ok(h) = std::env::var("JAVA_HOME") {
        let h = h.trim().trim_matches('"').to_string();
        if !h.is_empty() && std::path::Path::new(&h).join("bin").exists() {
            homes.insert(h);
        }
    }
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
    /// 任务类别（install/uninstall）：stage 完成时会被置为 done，
    /// 区分安装/卸载要用这个字段
    pub op: String,
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
    /// 追加任务日志（修复中心进度区展示；上限 200 条自动截断）。
    pub fn log(&self, msg: impl Into<String>) {
        let msg = msg.into();
        self.patch(move |j| j.logs.push(msg));
    }
}

/// 环境检查全量缓存类型（修复完成后清空，让下一次 /api/env 重新实测而非用 60s 旧缓存）。
pub type EnvCache = Arc<Mutex<Option<(std::time::Instant, Vec<crate::ops::environment::ServiceEnvReport>)>>>;

/// 平台管理的 JDK 免安装目录（zip 解压即用，无 MSI 产品、无注册表残留；
/// 与 mysql/redis 的 C:\services 同风格）。检测优先级最高：有 bin\java.exe 即达标。
pub const JDK_MANAGED_HOME: &str = "C:\\services\\jdk";

/// 免安装版 JDK 的默认最低大版本（一键修复对 java 的预期，与注册表最低声明一致）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub const JDK_DEFAULT_MAJOR: u32 = 8;

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
fn mysql_data_initialized(home: &str) -> bool {
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
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let out_file = backup_dir.join(format!("mysql-biz-{}.sql", timestamp_utc(secs)));
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
        // 非 MSI 条目（exe 卸载器）不进 java 卸载管线
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
        // 未初始化：data 存在但无系统库（比如 initialize 中途中断）
        let partial = std::env::temp_dir().join(format!("dfy-mysql-partial-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&partial);
        std::fs::create_dir_all(partial.join("data")).unwrap();
        assert!(!mysql_data_initialized(partial.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&tmp);
        let _ = std::fs::remove_dir_all(&partial);
    }

    /// 备份链路：mysqldump 候选顺序（平台目录顶层 → 嵌套 → MSI 常见位 → PATH）
    /// 与时间戳格式（备份文件名可读、按时间排序）。
    #[test]
    fn mysql_backup_helpers() {
        let cands = mysqldump_candidates(r"C:\services\mysql");
        // 首位 = 平台目录下的 bin/mysqldump.exe（分隔符由 Path::join 决定，
        // Windows 反斜杠、其余平台原样，均能被 is_file 正确识别）
        assert!(cands[0].starts_with(r"C:\services\mysql"));
        assert!(cands[0].ends_with("mysqldump.exe") && cands[0].contains("bin"));
        assert!(cands.iter().any(|p| p.contains("MySQL Server 8.0")));
        assert_eq!(cands.last().unwrap(), "mysqldump.exe", "PATH 兜底必须最后");

        assert_eq!(timestamp_utc(0), "19700101-000000");
        assert_eq!(timestamp_utc(1_700_000_000), "20231114-221320");
        // 轮转边界：一天末尾 +1 秒滚到次日
        assert_eq!(timestamp_utc(86399), "19700101-235959");
        assert_eq!(timestamp_utc(86400), "19700102-000000");
    }
}
