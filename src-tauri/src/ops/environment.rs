use std::path::PathBuf;
use std::process::Command;

use serde::Serialize;

use crate::ops::health;
use crate::ops::registry::{ServiceDef, ServiceKind};

/// 单项环境检查的结论。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// 满足要求
    Pass,
    /// 未满足但非致命（可降级/稍后处理）
    Warn,
    /// 致命缺失（该服务将无法运行）
    Fail,
}

/// 一条环境检查项。
#[derive(Debug, Clone, Serialize)]
pub struct CheckItem {
    /// 检查项名称（如 “Java 运行时”）
    pub name: String,
    /// 分类（运行时 / 外部依赖）
    pub category: String,
    pub status: CheckStatus,
    /// 期望（如 “JRE 8+”）
    pub expected: String,
    /// 实际探测结果（命令输出或端口状态）
    pub actual: String,
    /// 是否为致命项（缺失即无法运行）
    #[serde(rename = "isFatal")]
    pub is_fatal: bool,
    /// 缺失时的修复建议（面向 Windows 的 winget/choco/下载地址等，供用户在运维中心自助修复）
    #[serde(rename = "fixHint")]
    pub fix_hint: String,
}

/// 单个服务的环境检查报告。
#[derive(Debug, Clone, Serialize)]
pub struct ServiceEnvReport {
    pub id: String,
    pub name: String,
    /// 总体结论
    pub overall: CheckStatus,
    pub items: Vec<CheckItem>,
}

/// 执行命令并取首行有效输出（兼容 java/nginx 输出到 stderr 的情况）。
fn probe(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let text = if stdout.trim().is_empty() {
        String::from_utf8_lossy(&out.stderr)
    } else {
        stdout
    };
    text.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

/// 根据缺失项名称返回面向 Windows 的修复建议（winget / choco / 下载地址等）。
/// 仅当该项未通过时才会展示，供用户在运维中心自助修复。
/// 部署产物（jar / 前端 dist / Rust 二进制 / Tauri exe）均已预置，故修复建议只针对运行时依赖，
/// 且一律为“原生程序”安装方式（不依赖 Docker）。
fn fix_hint_for(name: &str) -> &'static str {
    match name {
        "Java 运行时" => "winget install -e --id Microsoft.OpenJDK.17   # 或访问 https://adoptium.net 安装 JRE 17",
        "MySQL（业务库）" => "winget install -e --id Oracle.MySQL   # 或访问 https://dev.mysql.com/downloads/ 安装 MySQL Server",
        "Redis（缓存）" => "winget install -e --id Redis.Redis   # 或：choco install redis-64",
        "nginx 托管" => "winget install -e --id nginx.nginx   # 或：choco install nginx，并配置站点指向构建产物",
        "GreptimeDB 二进制" => "从 https://github.com/GreptimeTeam/greptimedb/releases 下载 greptime 二进制，放入 install_dir/greptimedb/",
        "VerneMQ (MQTT)" => "从 https://vernemq.com/downloads 或 https://github.com/vernemq/vernemq/releases 安装 VerneMQ",
        "上游 DFY 后端" => "确认 dfy-backend 已启动并监听 127.0.0.1:48085",
        "WebView2 运行时" => "在目标 Windows 主机安装 Microsoft Edge WebView2 Runtime：https://developer.microsoft.com/microsoft-edge/webview2/",
        _ => "请在运维中心根据上方“实际”信息排查并安装对应依赖",
    }
}

fn make_item(
    name: &str,
    category: &str,
    expected: &str,
    actual: &str,
    pass: bool,
    fatal: bool,
) -> CheckItem {
    let status = if pass {
        CheckStatus::Pass
    } else if fatal {
        CheckStatus::Fail
    } else {
        CheckStatus::Warn
    };
    CheckItem {
        name: name.into(),
        category: category.into(),
        status,
        expected: expected.into(),
        actual: actual.into(),
        is_fatal: fatal,
        fix_hint: if pass { String::new() } else { fix_hint_for(name).into() },
    }
}

fn tcp_ok(port: u16) -> bool {
    health::check_tcp("127.0.0.1", port, 800)
}

/// 检测 WebView2 Runtime 是否已安装（Evergreen Bootstrap 注册表约定键）。
/// 键：HKLM/HKCU 下 SOFTWARE(\\WOW6432Node)\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5} 的 pv 值。
/// 64 位进程通过 KEY_WOW64_32KEY 覆盖 32 位视图（WOW6432Node 即其映射），两种写法都探测。
#[cfg(target_os = "windows")]
fn detect_webview2() -> Option<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY};
    use winreg::RegKey;

    const GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    const SUBKEYS: [&str; 2] = [
        r"SOFTWARE\Microsoft\EdgeUpdate\Clients",
        r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients",
    ];
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for sub in SUBKEYS {
            for flags in [KEY_READ, KEY_READ | KEY_WOW64_32KEY] {
                if let Ok(k) = RegKey::predef(hive).open_subkey_with_flags(&format!(r"{sub}\{GUID}"), flags) {
                    if let Ok(pv) = k.get_value::<String, _>("pv") {
                        if !pv.is_empty() {
                            return Some(pv);
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn detect_webview2() -> Option<String> {
    None
}

/// 检查某个被管服务的【运行时】环境。
///
/// 范围限定：仅探测“能否跑起来”所需的运行时依赖，**不含任何构建工具链**
/// （Maven / Cargo / Angular CLI / Tauri CLI / pnpm / npm / 构建用 Node 等）。
/// 部署产物（jar / 前端 dist / Rust 二进制 / Tauri exe）均已预置可直接运行；
/// 外部依赖一律按“原生程序”探测（端口可达 / 二进制存在 / 命令可用），不依赖容器。
pub fn check_service_env(def: &ServiceDef) -> ServiceEnvReport {
    let mut items: Vec<CheckItem> = Vec::new();

    match def.kind {
        ServiceKind::Vertx => {
            // Java 后端：运行需 JRE，以及 MySQL / Redis 依赖。
            let java = probe("java", &["-version"]);
            items.push(make_item(
                "Java 运行时",
                "运行时",
                "JRE 8+",
                java.as_deref().unwrap_or("未检测到"),
                java.is_some(),
                true,
            ));
            let mysql = tcp_ok(3306);
            items.push(make_item(
                "MySQL（业务库）",
                "外部依赖",
                "127.0.0.1:3306 可达",
                if mysql { "端口可连接" } else { "端口不可达" },
                mysql,
                false,
            ));
            let redis = tcp_ok(6379);
            items.push(make_item(
                "Redis（缓存）",
                "外部依赖",
                "127.0.0.1:6379 可达",
                if redis { "端口可连接" } else { "端口不可达" },
                redis,
                false,
            ));
        }
        ServiceKind::VueSpa => {
            // Vue3 前端：dist 已由 nginx 托管，运行时只需 nginx 在 :80 提供服务。
            let nginx = probe("nginx", &["-v"]);
            items.push(make_item(
                "nginx 托管",
                "外部依赖",
                "已安装并配置 :80",
                nginx.as_deref().unwrap_or("未检测到"),
                nginx.is_some(),
                false,
            ));
        }
        ServiceKind::AngularSpa => {
            // Angular 前端：dist 已由 nginx 托管，运行时只需 nginx 在 :8081 提供服务。
            let nginx = probe("nginx", &["-v"]);
            items.push(make_item(
                "nginx 托管",
                "外部依赖",
                "已安装并配置 :8081",
                nginx.as_deref().unwrap_or("未检测到"),
                nginx.is_some(),
                false,
            ));
        }
        ServiceKind::RustBin => {
            // Rust 边缘服务：预编译二进制已就绪，运行时需 GreptimeDB 二进制、VerneMQ、上游后端。
            let mut greptime_path = PathBuf::from(&def.install_dir);
            greptime_path.push("greptimedb");
            greptime_path.push("greptime");
            let greptime_ok =
                greptime_path.exists() || greptime_path.with_extension("exe").exists();
            items.push(make_item(
                "GreptimeDB 二进制",
                "外部依赖",
                "install_dir/greptimedb/greptime 存在",
                if greptime_ok {
                    "二进制存在"
                } else {
                    "未找到（时序数据功能将不可用）"
                },
                greptime_ok,
                false,
            ));
            let mqtt = tcp_ok(18830);
            items.push(make_item(
                "VerneMQ (MQTT)",
                "外部依赖",
                "127.0.0.1:18830 可达",
                if mqtt { "端口可连接" } else { "端口不可达" },
                mqtt,
                false,
            ));
            let upstream = tcp_ok(48085);
            items.push(make_item(
                "上游 DFY 后端",
                "外部依赖",
                "127.0.0.1:48085 可达",
                if upstream { "端口可连接" } else { "端口不可达" },
                upstream,
                false,
            ));
        }
        ServiceKind::TauriApp => {
            // Tauri 桌面客户端：exe 已预置，目标机运行时只需 WebView2。
            let ver = detect_webview2();
            let (actual, pass) = match ver.as_deref() {
                Some(v) => (format!("已安装 WebView2 Runtime {v}"), true),
                None => ("未检测到（Windows 主机需安装 Edge WebView2 Runtime）".into(), false),
            };
            items.push(make_item(
                "WebView2 运行时",
                "运行时",
                "WebView2 Runtime",
                &actual,
                pass,
                false,
            ));
        }
    }

    let overall = if items.iter().any(|i| i.status == CheckStatus::Fail) {
        CheckStatus::Fail
    } else if items.iter().any(|i| i.status == CheckStatus::Warn) {
        CheckStatus::Warn
    } else {
        CheckStatus::Pass
    };

    ServiceEnvReport {
        id: def.id.clone(),
        name: def.name.clone(),
        overall,
        items,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::registry;

    /// 直接驱动环境检查逻辑，对全部被管服务跑一遍并打印报告，
    /// 用于验证 runtime-only 探测是否按预期覆盖各服务的运行时依赖。
    #[test]
    fn check_all_services_env_and_print() {
        for def in registry::services() {
            let report = check_service_env(&def);
            let json = serde_json::to_string_pretty(&report).expect("serialize report");
            println!("=== 环境检查报告: {} ({}) ===\n{}\n", def.id, def.name, json);
        }
    }
}
