//! JDK 检测与环境变量管理（Windows 注册表 + macOS java_home）。
//!
//! 探测口径：PATH java 是第一事实；已装 JDK 清单只在 PATH 不达标时才盘点。

#[cfg(target_os = "windows")]
use super::{JDK_MANAGED_HOME};


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
pub(super) fn set_user_java_home(home: &str) -> Result<(), String> {
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
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let mut homes: BTreeSet<String> = BTreeSet::new();
    // 平台免安装目录优先：我们的 zip 管线装的 JDK（无注册表产品，检测必须走目录）
    if let Some(home) = managed_java_home() {
        homes.insert(home);
    }
    // 注册表实时读 JAVA_HOME/PATH：用户手动安装 JDK 后配置的环境变量写在注册表里，
    // 而本进程的环境变量是启动时快照——不重启平台 env::var 永远看不见。注册表
    // 是配置的事实源，必须实时读，否则「手动装 + 手动配」的机器检测不符。
    for (hive, sub) in [
        (
            HKEY_CURRENT_USER,
            r"Environment",
        ),
        (
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
    ] {
        let Ok(k) = RegKey::predef(hive).open_subkey(sub) else {
            continue;
        };
        if let Ok(h) = k.get_value::<String, _>("JAVA_HOME") {
            let h = h.trim().trim_matches('"').to_string();
            if !h.is_empty() {
                homes.insert(h);
            }
        }
        if let Ok(p) = k.get_value::<String, _>("Path") {
            for seg in p.split(';') {
                let s = seg.trim().trim_matches('"');
                if s.is_empty() || !std::path::Path::new(s).join("java.exe").is_file() {
                    continue;
                }
                // probe_home_version 需要 {home}\bin\java.exe；PATH 段常是 ...\bin，归一化到父目录
                let home = if s.rsplit('\\').next().map(|x| x.eq_ignore_ascii_case("bin")).unwrap_or(false) {
                    std::path::Path::new(s)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| s.to_string())
                } else {
                    s.to_string()
                };
                homes.insert(home);
            }
        }
    }
    // JAVA_HOME 兜底（进程快照；平台重启后本进程能看到新配置，注册表读失败时兜底）：
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
        assert_eq!(paths.len(), 3, "应提取 3 个 JVM Home 路径: {paths:?}");
        assert!(paths[0].ends_with("jbr-21.0.8/Contents/Home"));
        assert!(paths[1].contains("openjdk@17"));
    }
}
