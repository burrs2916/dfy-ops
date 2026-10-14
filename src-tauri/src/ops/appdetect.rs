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
