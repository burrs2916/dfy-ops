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
        "运行方式" => "托管部署：winget install -e --id nginx.nginx 并配置站点指向构建产物；dev 运行：从 https://nodejs.org 安装 Node.js",
        "VerneMQ 服务状态" => "在主机安装 VerneMQ（https://vernemq.com/downloads）并作为服务启动，MQTT 监听端口按部署配置（本机 18830）",
        "MinIO 服务状态" => "从 https://min.io/download 下载 minio 并作为服务运行（API 9000），配置 MINIO_ROOT_USER / MINIO_ROOT_PASSWORD",
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

/// 检查某个被管服务的【运行时】环境。
///
/// 范围限定：仅探测“能否跑起来”所需的运行时依赖，**不含任何构建工具链**
/// （Maven / Cargo / Angular CLI / Tauri CLI / pnpm / npm / 构建用 Node 等）。
/// 部署产物（jar / 前端 dist / Rust 二进制 / Tauri exe）均已预置可直接运行；
/// 外部依赖一律按“原生程序”探测（端口可达 / 二进制存在 / 命令可用），不依赖容器。
///
/// `running`：服务当前是否实测在运行（进程级检测，与状态接口同一事实来源）。
/// **服务在运行 ⇒ 环境事实上满足**——“能否跑起来”已被事实回答，此时跳过
/// 拓扑假设类检查（nginx 托管 / WebView2 等“部署形态”预设），只保留事实型
/// 外部依赖探测（MySQL / Redis / MQTT 等，运行中也可能挂，warn 是真实的）。
/// 平台不替服务决定部署形态：前端既可以是 nginx 托管、也可以是 dev server，
/// 任一可行路径存在即满足。
pub fn check_service_env(def: &ServiceDef, running: bool) -> ServiceEnvReport {
    let mut items: Vec<CheckItem> = Vec::new();

    match def.kind {
        ServiceKind::Vertx => {
            // Java 后端：运行需 JRE，以及 MySQL / Redis 依赖。
            // 三项均为事实型探测，运行中也照常检查（依赖可能中途挂掉）。
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
        ServiceKind::VueSpa | ServiceKind::AngularSpa => {
            // 前端不预设部署形态：nginx 托管（部署机）与 Node dev server（开发机）
            // 都是合法运行方式，任一可行路径存在即满足；服务在跑则事实已回答。
            if running {
                items.push(make_item(
                    "运行环境",
                    "运行时",
                    "具备可运行条件",
                    "服务实测运行中（进程级检测）",
                    true,
                    false,
                ));
            } else {
                let nginx = probe("nginx", &["-v"]);
                let node = probe("node", &["-v"]);
                let ok = nginx.is_some() || node.is_some();
                let actual = format!(
                    "nginx: {}；Node.js: {}",
                    nginx.as_deref().unwrap_or("未检测到"),
                    node.as_deref().unwrap_or("未检测到")
                );
                items.push(make_item(
                    "运行方式",
                    "运行时",
                    "nginx（托管部署）或 Node.js（dev 运行）任一可用",
                    &actual,
                    ok,
                    false,
                ));
            }
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
        ServiceKind::MqttBroker | ServiceKind::ObjectStorage => {
            // 第三方中间件（VerneMQ / MinIO）：它本身就是外部依赖，平台只检测不托管。
            // 运行中 → 事实上满足；未运行 → 提示在主机安装并启动（部署机）或检查容器。
            let label = match def.kind {
                ServiceKind::MqttBroker => "VerneMQ 服务",
                _ => "MinIO 服务",
            };
            items.push(make_item(
                &format!("{label}状态"),
                "外部依赖",
                "已安装并处于运行状态",
                if running {
                    "服务运行中（进程/端口检测）"
                } else {
                    "未检测到运行"
                },
                running,
                false,
            ));
        }
        ServiceKind::TauriApp => {
            // Tauri 桌面客户端：exe 已预置。WebView2 是 Windows 专属依赖；
            // macOS / Linux 使用系统内置 WebView，不存在该检查项。
            if running {
                items.push(make_item(
                    "运行环境",
                    "运行时",
                    "具备可运行条件",
                    "服务实测运行中（进程级检测）",
                    true,
                    false,
                ));
            } else {
                #[cfg(target_os = "windows")]
                {
                    let ver = detect_webview2();
                    let (actual, pass) = match ver.as_deref() {
                        Some(v) => (format!("已安装 WebView2 Runtime {v}"), true),
                        None => ("未检测到（需安装 Edge WebView2 Runtime）".into(), false),
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
                #[cfg(not(target_os = "windows"))]
                {
                    items.push(make_item(
                        "WebView 运行时",
                        "运行时",
                        "系统内置 WebView",
                        "macOS / Linux 使用系统内置 WebView（WKWebView / webkit2gtk），无需额外安装",
                        true,
                        false,
                    ));
                }
            }
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
            let report = check_service_env(&def, false);
            let json = serde_json::to_string_pretty(&report).expect("serialize report");
            println!("=== 环境检查报告: {} ({}) ===\n{}\n", def.id, def.name, json);
        }
    }

    /// 运行中的服务：环境事实上满足（能否跑起来已被事实回答），
    /// 不得因“nginx 未装 / WebView2 未检测到”这类拓扑假设产生 warn。
    /// 注意不含 dfy-backend——其 Java/MySQL/Redis 是事实型检查，
    /// 在没装这些依赖的机器上 fail 是正确行为（如 CI 环境）。
    #[test]
    fn running_service_env_is_pass() {
        for id in ["dfy-ui", "gateway-console", "biosphere-spatial"] {
            let def = registry::services().into_iter().find(|d| d.id == id).unwrap();
            let report = check_service_env(&def, true);
            assert_eq!(
                report.overall,
                CheckStatus::Pass,
                "运行中的 {id} 环境检查不应告警: {:?}",
                report.items
            );
        }
    }

    /// 未运行的前端：不预设部署形态 —— nginx 或 Node.js 任一可用即 pass。
    /// 在本机（装了 Node、没装 nginx）上应判定通过。
    #[test]
    fn stopped_frontend_env_accepts_node_or_nginx() {
        let def = registry::services()
            .into_iter()
            .find(|d| d.id == "gateway-console")
            .unwrap();
        let report = check_service_env(&def, false);
        let has_runtime = report
            .items
            .iter()
            .any(|i| i.name == "运行方式" && i.status == CheckStatus::Pass);
        assert!(
            has_runtime || report.items.iter().all(|i| i.status == CheckStatus::Pass),
            "未运行前端的环境检查应给出运行方式判定: {:?}",
            report.items
        );
    }

    /// macOS：Tauri 客户端使用系统内置 WKWebView，未运行也不应产生 WebView2 假警告。
    #[cfg(target_os = "macos")]
    #[test]
    fn tauri_env_passes_on_macos_without_running() {
        let def = registry::services()
            .into_iter()
            .find(|d| d.id == "biosphere-spatial")
            .unwrap();
        let report = check_service_env(&def, false);
        assert_eq!(report.overall, CheckStatus::Pass);
    }
}
