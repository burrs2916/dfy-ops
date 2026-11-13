//! 「已安装」检测：与部署路径无关，读 Windows 卸载注册表（与控制面板
//! 「程序和功能」同一数据源）。NSIS/MSI 安装器（含 Tauri 打包）安装完成
//! 必写 Uninstall 键；没装过就没有键——这是「装没装」的事实来源。
//!
//! 匹配策略（组合，避免单一依据失效）：`DisplayName` 含产品名 token
//! （如"空间数据驱动"），或 `DisplayIcon` / `InstallLocation` 含主程序
//! exe 基名。任一命中即判定已安装。

/// 是否已安装（Windows 读卸载注册表）。
/// `product_tokens`：产品显示名子串（大小写不敏感）；`exe_names`：主程序 exe 基名。
#[cfg(target_os = "windows")]
pub fn is_installed(product_tokens: &[String], exe_names: &[String]) -> bool {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY};
    use winreg::RegKey;

    const ROOTS: [&str; 2] = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];
    let tokens_lc: Vec<String> = product_tokens.iter().map(|t| t.to_lowercase()).collect();
    let exe_lc: Vec<String> = exe_names.iter().map(|e| e.to_lowercase()).collect();
    if tokens_lc.is_empty() && exe_lc.is_empty() {
        return false;
    }
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for root in ROOTS {
            // 64 位与 32 位视图都查：32 位安装器可能只写 WOW6432Node，
            // HKCU 则是 per-user 安装（NSIS 默认 current-user 模式）的唯一落点
            for flags in [KEY_READ, KEY_READ | KEY_WOW64_32KEY] {
                let Ok(uninst) = RegKey::predef(hive).open_subkey_with_flags(root, flags) else {
                    continue;
                };
                for key_name in uninst.enum_keys().flatten() {
                    let Ok(k) = uninst.open_subkey_with_flags(&key_name, flags) else {
                        continue;
                    };
                    let display: String = k.get_value("DisplayName").unwrap_or_default();
                    let icon: String = k.get_value("DisplayIcon").unwrap_or_default();
                    let location: String = k.get_value("InstallLocation").unwrap_or_default();
                    let display_lc = display.to_lowercase();
                    let icon_lc = icon.to_lowercase();
                    let location_lc = location.to_lowercase();
                    let hit_name =
                        !display.is_empty() && tokens_lc.iter().any(|t| display_lc.contains(t));
                    let hit_exe = exe_lc
                        .iter()
                        .any(|e| icon_lc.contains(e) || location_lc.contains(e));
                    if hit_name || hit_exe {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// 非 Windows（开发机）：「已安装」是 Windows 部署语义（卸载注册表），
/// 恒为 false——运行检测（pgrep -f）不受影响，跑起来仍如实显示运行中。
#[cfg(not(target_os = "windows"))]
pub fn is_installed(_product_tokens: &[String], _exe_names: &[String]) -> bool {
    false
}

/// 卸载注册表条目详情（启动/卸载链路共用）：主程序位置 + 卸载命令。
/// 与 is_installed 同一数据源（控制面板「程序和功能」），按产品显示名命中。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct UninstallEntry {
    /// InstallLocation（安装目录，可能为空）
    pub install_location: String,
    /// DisplayIcon（主程序 exe 路径，可能为空）
    pub display_icon: String,
    /// QuietUninstallString（静默卸载整条命令，可能为空）
    pub quiet_cmd: String,
    /// UninstallString（普通卸载命令，可能为空）
    pub uninstall_cmd: String,
}

/// 按产品显示名取卸载注册表条目；未安装返回 None。
#[cfg(target_os = "windows")]
pub fn uninstall_entry(product_tokens: &[String]) -> Option<UninstallEntry> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY};
    use winreg::RegKey;

    const ROOTS: [&str; 2] = [
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];
    let tokens_lc: Vec<String> = product_tokens.iter().map(|t| t.to_lowercase()).collect();
    if tokens_lc.is_empty() {
        return None;
    }
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for root in ROOTS {
            // 64/32 位视图 × HKLM/HKCU 全枚举：NSIS 默认 per-user 落 HKCU
            for flags in [KEY_READ, KEY_READ | KEY_WOW64_32KEY] {
                let Ok(uninst) = RegKey::predef(hive).open_subkey_with_flags(root, flags) else {
                    continue;
                };
                for key_name in uninst.enum_keys().flatten() {
                    let Ok(k) = uninst.open_subkey_with_flags(&key_name, flags) else {
                        continue;
                    };
                    let display: String = k.get_value("DisplayName").unwrap_or_default();
                    let display_lc = display.to_lowercase();
                    if display.is_empty() || !tokens_lc.iter().any(|t| display_lc.contains(t)) {
                        continue;
                    }
                    return Some(UninstallEntry {
                        install_location: k.get_value("InstallLocation").unwrap_or_default(),
                        display_icon: k.get_value("DisplayIcon").unwrap_or_default(),
                        quiet_cmd: k.get_value("QuietUninstallString").unwrap_or_default(),
                        uninstall_cmd: k.get_value("UninstallString").unwrap_or_default(),
                    });
                }
            }
        }
    }
    None
}

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
#[cfg(not(target_os = "windows"))]
pub fn uninstall_entry(_product_tokens: &[String]) -> Option<UninstallEntry> {
    None
}

/// 已安装的主程序 exe 路径（启动链路用）：DisplayIcon（去引号与 ,0 序号后缀）
/// 优先；其次 InstallLocation + 进程名候选（含 .exe 变体）。找不到返回 None。
#[cfg(target_os = "windows")]
pub fn installed_exe(product_tokens: &[String], process_names: &[String]) -> Option<std::path::PathBuf> {
    let entry = uninstall_entry(product_tokens)?;
    let icon = entry.display_icon.trim().trim_matches('"');
    // DisplayIcon 形如 "C:\...\app.exe,0"——去掉 ",序号" 后缀
    let icon_path = icon.split(',').next().unwrap_or("").trim();
    if !icon_path.is_empty() {
        let p = std::path::PathBuf::from(icon_path);
        if p.extension().map(|e| e.eq_ignore_ascii_case("exe")).unwrap_or(false) && p.is_file() {
            return Some(p);
        }
    }
    let loc = entry.install_location.trim().trim_matches('"').trim_end_matches('\\');
    if loc.is_empty() {
        return None;
    }
    for name in process_names {
        let base = name.trim().trim_matches('"');
        let candidates = [base.to_string(), format!("{base}.exe")];
        for c in candidates {
            let p = std::path::Path::new(loc).join(c);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn installed_exe(
    _product_tokens: &[String],
    _process_names: &[String],
) -> Option<std::path::PathBuf> {
    None
}

/// 应用进程是否在跑（tasklist 按映像名，含 .exe 变体）。
/// 环境检查运行态 / 启停复核共用——与卸载、启动同源，杜绝两套判定。
pub fn is_app_running(process_names: &[String]) -> bool {
    #[cfg(target_os = "windows")]
    {
        for name in process_names {
            let base = name.trim().trim_matches('"');
            for cand in [base.to_string(), format!("{base}.exe")] {
                let hit = std::process::Command::new("tasklist")
                    .args(["/fi", &format!("imagename eq {cand}"), "/fo", "csv", "/nh"])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.contains(&cand)))
                    .unwrap_or(false);
                if hit {
                    return true;
                }
            }
        }
        false
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = process_names;
        false
    }
}

/// 结束应用进程（taskkill /IM /T /F，含 .exe 变体）。返回是否执行过结束动作
/// （进程本来就没跑 = false，调用方据此写日志）。卸载前置关闭 / 停止按钮共用。
pub fn kill_app_processes(process_names: &[String]) -> bool {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut acted = false;
        for name in process_names {
            let base = name.trim().trim_matches('"');
            for cand in [base.to_string(), format!("{base}.exe")] {
                let out = std::process::Command::new("taskkill")
                    .args(["/IM", &cand, "/T", "/F"])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output();
                // 非零退出通常 = 该映像没在跑（ERROR 1287），不算失败
                if out.as_ref().map(|o| o.status.success()).unwrap_or(false) {
                    acted = true;
                }
            }
        }
        acted
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = process_names;
        false
    }
}
