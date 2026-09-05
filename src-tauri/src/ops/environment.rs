use std::collections::HashMap;
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
    /// 稳定 id（修复建议键控 / 前端键控），不随显示名变化
    pub id: String,
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
    /// 缺失时的修复建议（按当前平台生成，供用户在运维中心自助修复）
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

/// 依赖服务的实时状态视图（由调用方从 to_status 构建——与环境检查共用
/// 「进程是事实来源」的同一份检测结果，杜绝端口/路径硬编码）。
#[derive(Debug, Clone)]
pub struct DepState {
    pub name: String,
    pub running: bool,
    pub healthy: bool,
    pub ports: Vec<u16>,
}

/// 服务 id -> 实时状态。
pub type DepStates = HashMap<String, DepState>;

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

/// 从 `java -version` 输出解析 (major, minor, patch)，兼容旧式 "1.8.0_382"。
fn java_version_tuple(output: &str) -> Option<(u32, u32, u32)> {
    let start = output.find('"')? + 1;
    let end = output[start..].find('"')? + start;
    let mut it = output[start..end].split('.');
    let a: u32 = it.next()?.parse().ok()?;
    let b: u32 = it.next().and_then(|s| s.split('_').next()).and_then(|s| s.parse().ok()).unwrap_or(0);
    let c: u32 = it
        .next()
        .and_then(|s| s.split('_').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    // 旧版本方案 major=1，真实大版本在 minor（1.8 = Java 8）
    if a == 1 {
        Some((b, c, 0))
    } else {
        Some((a, b, c))
    }
}

/// 按检查项 id 返回修复建议；按当前平台生成（Windows 用 winget/choco，
/// macOS 用 brew / 官方下载链接——dev 机上看到 winget 是噪音）。
fn fix_hint_for(id: &str) -> String {
    // (windows 提示, 非 windows 提示)
    let by_platform = |w: &str, m: &str| {
        if cfg!(target_os = "windows") {
            w.to_string()
        } else {
            m.to_string()
        }
    };
    match id {
        "java" => by_platform(
            "winget install -e --id Microsoft.OpenJDK.17   # 或访问 https://adoptium.net 安装 JRE 17",
            "brew install --cask temurin@17   # 或访问 https://adoptium.net",
        ),
        "runtime-path" => by_platform(
            "托管部署：winget install -e --id nginx.nginx；dev 运行：从 https://nodejs.org 安装 Node.js",
            "托管部署：brew install nginx；dev 运行：brew install node",
        ),
        "greptime" => "从 https://github.com/GreptimeTeam/greptimedb/releases 下载 greptime 二进制，放入 install_dir/greptimedb/".into(),
        "webview2" => "在目标 Windows 主机安装 Microsoft Edge WebView2 Runtime：https://developer.microsoft.com/microsoft-edge/webview2/".into(),
        "service-vernemq" | "dep-vernemq" => "在主机安装 VerneMQ（https://vernemq.com/downloads）并作为服务启动；本机开发可用 OrbStack/Docker 容器提供".into(),
        "service-minio" | "dep-minio" => "从 https://min.io/download 下载 minio 并作为服务运行，配置 MINIO_ROOT_USER / MINIO_ROOT_PASSWORD；本机开发可用容器".into(),
        "service-mysql" | "dep-mysql" => by_platform(
            "winget install -e --id Oracle.MySQL   # 或访问 https://dev.mysql.com/downloads/ 安装 MySQL Server",
            "brew install mysql   # 或用 OrbStack/Docker 容器提供",
        ),
        "service-redis" | "dep-redis" => by_platform(
            "winget install -e --id Redis.Redis   # 或：choco install redis-64",
            "brew install redis   # 或用 OrbStack/Docker 容器提供",
        ),
        "dep-dfy-backend" => "在服务总览启动 DFY 后端（受管），或确认其已在外部运行".into(),
        _ => "请在运维中心根据上方“实际”信息排查并安装对应依赖".into(),
    }
}

/// 组装一条检查项：pass 时清空 hint，非 pass 按平台生成。
fn item(
    id: &str,
    name: &str,
    category: &str,
    expected: &str,
    actual: &str,
    status: CheckStatus,
) -> CheckItem {
    CheckItem {
        id: id.into(),
        name: name.into(),
        category: category.into(),
        status: status.clone(),
        expected: expected.into(),
        actual: actual.into(),
        is_fatal: status == CheckStatus::Fail,
        fix_hint: if status == CheckStatus::Pass {
            String::new()
        } else {
            fix_hint_for(id)
        },
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
/// 范围限定：仅探测“能否跑起来”所需的运行时依赖，**不含任何构建工具链**。
///
/// 检查模型（三段，全部声明驱动、零硬编码端口/路径）：
/// 1. **运行时检查**（kind 特有）：Java/WebView2/前端运行方式等；
/// 2. **requires 声明检查**（`ServiceDef.requires`）：路径/端口由配置声明；
/// 3. **dependsOn 依赖检查**：直接引用被依赖服务的实时状态（同一份「进程是事实来源」
///    检测结果），依赖未运行即 Fail（启动它没有意义）、运行但端口不可达即 Warn。
///
/// `states`：全部服务的实时状态视图（调用方从 to_status 构建）。
pub fn check_service_env(def: &ServiceDef, states: &DepStates) -> ServiceEnvReport {
    let mut items: Vec<CheckItem> = Vec::new();
    let running = states.get(&def.id).map(|s| s.running).unwrap_or(false);

    // ===== 1. 运行时检查（kind 特有） =====
    match def.kind {
        ServiceKind::Vertx => {
            // Java 运行时：探测并比对版本（期望 JRE 8+），运行中也照常检查。
            let java = probe("java", &["-version"]);
            let it = match java {
                None => item(
                    "java",
                    "Java 运行时",
                    "运行时",
                    "JRE 8+",
                    "未检测到",
                    CheckStatus::Fail,
                ),
                Some(out) => {
                    let status = match java_version_tuple(&out) {
                        Some((major, _, _)) if major >= 8 => CheckStatus::Pass,
                        Some(_) => CheckStatus::Warn,
                        None => CheckStatus::Warn,
                    };
                    let note = match java_version_tuple(&out) {
                        Some((major, _, _)) if major >= 8 => "（满足 JRE 8+）",
                        Some(_) => "（版本过低，需 JRE 8+）",
                        None => "（无法解析版本号）",
                    };
                    item(
                        "java",
                        "Java 运行时",
                        "运行时",
                        "JRE 8+",
                        &format!("{out}{note}"),
                        status,
                    )
                }
            };
            items.push(it);
        }
        ServiceKind::VueSpa | ServiceKind::AngularSpa => {
            // 前端不预设部署形态：未运行时检查可行运行路径（nginx 或 Node 任一可用）；
            // 运行中则事实已回答，交给依赖检查（如 dfy-ui → dfy-backend）。
            if !running {
                let nginx = probe("nginx", &["-v"]);
                let node = probe("node", &["-v"]);
                let ok = nginx.is_some() || node.is_some();
                let actual = format!(
                    "nginx: {}；Node.js: {}",
                    nginx.as_deref().unwrap_or("未检测到"),
                    node.as_deref().unwrap_or("未检测到")
                );
                items.push(item(
                    "runtime-path",
                    "运行方式",
                    "运行时",
                    "nginx（托管部署）或 Node.js（dev 运行）任一可用",
                    &actual,
                    if ok {
                        CheckStatus::Pass
                    } else {
                        CheckStatus::Warn
                    },
                ));
            }
        }
        ServiceKind::MqttBroker | ServiceKind::ObjectStorage | ServiceKind::Database => {
            // 第三方中间件（VerneMQ/MinIO/MySQL/Redis）：本身就是外部依赖，平台只检测不托管。
            if running {
                items.push(item(
                    "service-state",
                    "服务状态",
                    "外部依赖",
                    "已安装并处于运行状态",
                    "服务运行中（进程/端口检测）",
                    CheckStatus::Pass,
                ));
            } else {
                items.push(item(
                    &format!("service-{}", def.id),
                    "服务状态",
                    "外部依赖",
                    "已安装并处于运行状态",
                    "未检测到运行",
                    CheckStatus::Warn,
                ));
            }
        }
        ServiceKind::TauriApp => {
            // Tauri 桌面客户端：WebView2 是 Windows 专属依赖；运行中则事实已回答。
            if running {
                items.push(item(
                    "service-state",
                    "运行环境",
                    "运行时",
                    "具备可运行条件",
                    "服务实测运行中（进程级检测）",
                    CheckStatus::Pass,
                ));
            } else {
                #[cfg(target_os = "windows")]
                {
                    let ver = detect_webview2();
                    let status = if ver.is_some() {
                        CheckStatus::Pass
                    } else {
                        CheckStatus::Warn
                    };
                    let actual = ver
                        .map(|v| format!("已安装 WebView2 Runtime {v}"))
                        .unwrap_or_else(|| "未检测到（需安装 Edge WebView2 Runtime）".into());
                    items.push(item("webview2", "WebView2 运行时", "运行时", "WebView2 Runtime", &actual, status));
                }
                #[cfg(not(target_os = "windows"))]
                {
                    items.push(item(
                        "webview",
                        "WebView 运行时",
                        "运行时",
                        "系统内置 WebView",
                        "macOS / Linux 使用系统内置 WebView（WKWebView / webkit2gtk），无需额外安装",
                        CheckStatus::Pass,
                    ));
                }
            }
        }
        ServiceKind::RustBin => {
            // Rust 边缘服务：无 kind 特有运行时检查，依赖见 requires + dependsOn。
        }
    }

    // ===== 2. requires 声明检查（配置驱动，零硬编码） =====
    for req in &def.requires {
        let (ok, actual, expected) = match (&req.path, &req.port) {
            (Some(p), _) => {
                let path = PathBuf::from(&def.install_dir).join(p);
                let exists = path.exists() || path.with_extension("exe").exists();
                let disp = path.display().to_string();
                (
                    exists,
                    if exists {
                        format!("文件存在（{disp}）")
                    } else {
                        format!("未找到（{disp}）")
                    },
                    format!("{p} 存在"),
                )
            }
            (None, Some(port)) => {
                let ok = tcp_ok(*port);
                (
                    ok,
                    if ok {
                        format!("端口 {port} 可连接")
                    } else {
                        format!("端口 {port} 不可达")
                    },
                    format!("127.0.0.1:{port} 可达"),
                )
            }
            (None, None) => (false, "requires 配置缺少 path/port".into(), "有效配置".into()),
        };
        let status = if ok {
            CheckStatus::Pass
        } else {
            CheckStatus::Warn
        };
        items.push(item(req.check_id(), &req.label, "外部依赖", &expected, &actual, status));
    }

    // ===== 3. dependsOn 依赖检查（引用被依赖服务的实时状态，同一事实来源） =====
    for dep_id in &def.depends_on {
        let it = match states.get(dep_id) {
            Some(dep) => {
                let ports_txt = if dep.ports.is_empty() {
                    "无监听端口".to_string()
                } else {
                    dep.ports
                        .iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join("/")
                };
                let (status, actual) = if dep.running && dep.healthy {
                    (CheckStatus::Pass, format!("运行中，端口 {ports_txt}"))
                } else if dep.running {
                    // 依赖在跑但端口不可达：非致命，但功能可能受限
                    (CheckStatus::Warn, format!("运行中但端口不可达（{ports_txt}）"))
                } else {
                    // 依赖没跑：致命——先把它拉起来，启动本服务没有意义
                    (CheckStatus::Fail, "未运行".to_string())
                };
                item(
                    &format!("dep-{dep_id}"),
                    &format!("依赖：{}", dep.name),
                    "外部依赖",
                    "依赖服务运行且健康",
                    &actual,
                    status,
                )
            }
            None => item(
                &format!("dep-{dep_id}"),
                &format!("依赖：{dep_id}"),
                "外部依赖",
                "已在服务注册表中注册",
                "未在注册表中找到该依赖（检查 services.toml 的 dependsOn）",
                CheckStatus::Warn,
            ),
        };
        items.push(it);
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

    fn states(list: &[(&str, bool, bool, &[u16])]) -> DepStates {
        list.iter()
            .map(|(id, running, healthy, ports)| {
                (
                    id.to_string(),
                    DepState {
                        name: id.to_string(),
                        running: *running,
                        healthy: *healthy,
                        ports: ports.to_vec(),
                    },
                )
            })
            .collect()
    }

    fn def_of(id: &str) -> ServiceDef {
        registry::services()
            .into_iter()
            .find(|d| d.id == id)
            .unwrap_or_else(|| panic!("{id} 必须在注册表中"))
    }

    /// java -version 输出版本解析：新式/旧式/非法输出。
    #[test]
    fn java_version_parsing() {
        assert_eq!(
            java_version_tuple("openjdk version \"17.0.15\" 2025-04-15"),
            Some((17, 0, 15))
        );
        assert_eq!(
            java_version_tuple("java version \"1.8.0_382\""),
            Some((8, 0, 0))
        );
        assert_eq!(java_version_tuple("没有版本信息"), None);
    }

    /// 运行中的服务：拓扑假设项（nginx/WebView2）不产生 warn；
    /// 且依赖健康时（同一事实来源）整体 pass。
    #[test]
    fn running_service_env_is_pass() {
        let st = states(&[
            ("dfy-ui", true, true, &[]),
            ("gateway-console", true, true, &[4200]),
            ("biosphere-spatial", true, true, &[]),
            ("dfy-backend", true, true, &[48085]),
            ("mysql", true, true, &[3306]),
            ("redis", true, true, &[6379]),
        ]);
        for id in ["dfy-ui", "gateway-console", "biosphere-spatial", "dfy-backend"] {
            let def = def_of(id);
            let report = check_service_env(&def, &st);
            assert_eq!(
                report.overall,
                CheckStatus::Pass,
                "运行中的 {id}（依赖齐全）环境检查不应告警: {:?}",
                report.items
            );
        }
    }

    /// 依赖检查走 dependsOn + 实时状态视图：依赖未运行 → Fail（启动无意义）；
    /// 运行但端口不可达 → Warn；未注册 → Warn。全部由声明驱动，无硬编码端口。
    #[test]
    fn dependency_checks_driven_by_depends_on() {
        let def = def_of("dfy-backend");
        assert_eq!(def.depends_on, vec!["mysql", "redis"]);

        // MySQL 挂了 → fail；Redis 健康 → pass
        let st = states(&[
            ("dfy-backend", false, false, &[]),
            ("mysql", false, false, &[]),
            ("redis", true, true, &[6379]),
        ]);
        let report = check_service_env(&def, &st);
        let mysql = report.items.iter().find(|i| i.id == "dep-mysql").unwrap();
        let redis = report.items.iter().find(|i| i.id == "dep-redis").unwrap();
        assert_eq!(mysql.status, CheckStatus::Fail, "依赖未运行应为 fail（启动无意义）");
        assert_eq!(redis.status, CheckStatus::Pass);
        assert_eq!(report.overall, CheckStatus::Fail);

        // MySQL 在跑但端口不可达 → warn
        let st = states(&[
            ("dfy-backend", false, false, &[]),
            ("mysql", true, false, &[]),
            ("redis", true, true, &[6379]),
        ]);
        let report = check_service_env(&def, &st);
        let mysql = report.items.iter().find(|i| i.id == "dep-mysql").unwrap();
        assert_eq!(mysql.status, CheckStatus::Warn);
        assert_eq!(report.overall, CheckStatus::Warn);
    }

    /// 未运行的前端：不预设部署形态 —— nginx 或 Node.js 任一可用即 pass。
    #[test]
    fn stopped_frontend_env_accepts_node_or_nginx() {
        let def = def_of("gateway-console");
        let report = check_service_env(&def, &states(&[]));
        let has_runtime = report
            .items
            .iter()
            .any(|i| i.id == "runtime-path" && i.status == CheckStatus::Pass);
        assert!(
            has_runtime || report.items.iter().all(|i| i.status == CheckStatus::Pass),
            "未运行前端的环境检查应给出运行方式判定: {:?}",
            report.items
        );
    }

    /// requires 声明检查：GreptimeDB 由 ServiceDef.requires 驱动（不再硬编码在代码里）。
    #[test]
    fn requires_declarations_drive_checks() {
        let def = def_of("edge-conductor");
        assert!(def.requires.iter().any(|r| r.check_id() == "greptime"));
        // 未提供状态视图时 running=false，但 requires 照常探测
        let report = check_service_env(&def, &states(&[]));
        assert!(
            report.items.iter().any(|i| i.id == "greptime"),
            "requires 声明的检查项必须出现: {:?}",
            report.items
        );
    }

    /// macOS：Tauri 客户端使用系统内置 WKWebView，未运行也不应产生 WebView2 假警告。
    #[cfg(target_os = "macos")]
    #[test]
    fn tauri_env_passes_on_macos_without_running() {
        let def = def_of("biosphere-spatial");
        let report = check_service_env(&def, &states(&[]));
        assert_eq!(report.overall, CheckStatus::Pass);
    }
}
