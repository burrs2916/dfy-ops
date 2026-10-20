use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

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
    /// 纯远程第三方（remoteOnly，如 Redis）的托管端点：本机无进程/端口可查，
    /// running/healthy 已是「托管端点 TCP 可达」的探测结果，端点值用于生成
    /// 面向用户的提示文案（不可达时引导去连接配置）。
    pub remote: Option<(String, u16)>,
}

/// 服务 id -> 实时状态。
pub type DepStates = HashMap<String, DepState>;

/// 执行命令并取首行有效输出（兼容 java/nginx 输出到 stderr 的情况）。
///
/// 带硬超时（3s）：`java -version` 在杀软扫描/冷启动下可能显著变慢，
/// 损坏的运行时甚至可能挂起——无超时的 `output()` 会把整轮检测永久卡死。
fn probe(cmd: &str, args: &[&str]) -> Option<String> {
    const TIMEOUT: Duration = Duration::from_millis(3000);
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            // 超时：杀掉并视为探测失败（输出不再读取，进程死后管道自然 EOF）
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Err(_) => return None,
        }
    }?;
    if !status.success() {
        return None;
    }
    let mut stdout = String::new();
    if let Some(s) = child.stdout.as_mut() {
        let _ = s.read_to_string(&mut stdout);
    }
    let text = if stdout.trim().is_empty() {
        let mut stderr = String::new();
        if let Some(s) = child.stderr.as_mut() {
            let _ = s.read_to_string(&mut stderr);
        }
        stderr
    } else {
        stdout
    };
    text.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
}

/// 一次全量检查会话内共享的探测缓存：同一命令（java -version / nginx -v / node -v）
/// 只真正执行一次子进程调用，其余服务复用结果。
///
/// 全量检查并行化后，多个 Vertx 服务会同时要 `java -version`——不共享就是
/// N 次 JVM 冷启动（每次 0.3-1s）。锁内查、锁外探，并发重复探测天然收敛为一次。
#[derive(Default)]
pub struct ProbeCache {
    map: Mutex<HashMap<String, Option<String>>>,
}

impl ProbeCache {
    fn probe(&self, cmd: &str, args: &[&str]) -> Option<String> {
        let key = format!("{cmd}?{}", args.join(" "));
        if let Some(v) = self.map.lock().unwrap().get(&key) {
            return v.clone();
        }
        let v = probe(cmd, args);
        self.map.lock().unwrap().insert(key, v.clone());
        v
    }
}

/// 会话缓存的探测入口：cache=None 时直接探测（单服务检查量小，无需共享）。
fn probe_cached(cache: Option<&ProbeCache>, cmd: &str, args: &[&str]) -> Option<String> {
    match cache {
        Some(c) => c.probe(cmd, args),
        None => probe(cmd, args),
    }
}

/// 从 `java -version` 输出解析 (major, minor, patch)，兼容旧式 "1.8.0_382"。
pub(crate) fn java_version_tuple(output: &str) -> Option<(u32, u32, u32)> {
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

/// Java 运行时的修复命令：版本号来自服务声明（javaMajor），Windows 走 winget，
/// 其他平台用 Temurin（Adoptium）。fix_hint 必须与声明期望一致——dfy-boot-base
/// 按 JDK 8 编译，提示装 17 是错误引导（2026-09-07 修正）。
fn java_fix_hint(major: u32) -> String {
    if cfg!(target_os = "windows") {
        format!("winget install -e --id Microsoft.OpenJDK.{major}   # 或访问 https://adoptium.net 安装 JDK {major}")
    } else {
        format!("brew install --cask temurin@{major}   # 或访问 https://adoptium.net 安装 JDK {major}")
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
        "java" => java_fix_hint(8),
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
        "service-redis" | "dep-redis" => {
            "Redis 为远程部署模式（本机不安装）：请到「运维中心 → Redis 管理 → 连接配置」检查主机/端口/密码设置后重新检测".into()
        }
        "dep-dfy-backend" => "在服务总览启动 DFY 后端（受管），或确认其已在外部运行".into(),
        _ => "请在运维中心根据上方“实际”信息排查并安装对应依赖".into(),
    }
}

/// 探测本机是否已有 MySQL 安装痕迹（Windows 服务 / 安装目录 / MSI 条目），
/// 返回给用户看的来源描述。用于「未运行」时区分建议：启动现有安装 vs 安装新的。
#[cfg(target_os = "windows")]
fn detect_mysql_install() -> Option<String> {
    for svc in ["MySQL80", "MySQL84", "MySQL57", "MySQL"] {
        if crate::ops::env_repair::service_exists(svc) {
            return Some(format!("Windows 服务 {svc}"));
        }
    }
    // 平台 zip 安装目录（含嵌套布局：bin 在顶层或一层子目录）
    if let Ok(rd) = std::fs::read_dir("C:\\services\\mysql") {
        for e in rd.flatten() {
            let p = e.path();
            let bin = if p.is_dir() && p.join("bin").is_dir() {
                p.join("bin")
            } else {
                p.clone()
            };
            if bin.join("mysqld.exe").is_file() {
                return Some(format!("安装目录 {}", p.display()));
            }
        }
    }
    if !crate::ops::env_repair::mysql_msi_entries().is_empty() {
        return Some("MSI 安装条目（MySQL Installer）".to_string());
    }
    None
}

#[cfg(not(target_os = "windows"))]
fn detect_mysql_install() -> Option<String> {
    None
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

/// Java 检测（唯一口径 = 环境变量，安装版 JDK）：第一事实 = PATH 上的 java
/// （run_cmd 实际执行者）；PATH 不可用时盘点本机已装 JDK（注册表 JAVA_HOME/
/// PATH 实时读取，逐个实跑验证版本），区分「配置修复」与「需要安装」。
/// 不再认交付包内置 JRE。
fn push_path_java_check(items: &mut Vec<CheckItem>, expected_major: u32, cache: Option<&ProbeCache>) {
    let java = probe_cached(cache, "java", &["-version"]);
    let path_ver = java.as_deref().and_then(java_version_tuple);
    let it = if let Some((major, minor, patch)) = path_ver.filter(|(m, _, _)| *m >= expected_major) {
        item(
            "java",
            "Java 运行时",
            "运行时",
            &format!("JDK {expected_major}+"),
            &format!(
                "安装版 JDK {}.{}.{}（环境变量 PATH，满足 JDK {expected_major}+）",
                major, minor, patch
            ),
            CheckStatus::Pass,
        )
    } else {
        let installs = crate::ops::env_repair::installed_jdks();
        let compatible = installs
            .iter()
            .filter(|j| j.major() >= expected_major)
            .max_by_key(|j| j.version);
        let it = match (path_ver, compatible) {
            // 平台托管语义：免安装版 JDK + 用户级 JAVA_HOME 时 PATH 上没有 java
            // 是常态，因此本机已有实探达标的 JDK（注册表/常见目录/JAVA_HOME，
            // installed_jdks 内部逐个实跑验证过版本）即视为就绪，不再强求 PATH。
            (_, Some(j)) => item(
                "java",
                "Java 运行时",
                "运行时",
                &format!("JDK {expected_major}+"),
                &format!(
                    "本机已安装 JDK {}（{}），满足 JDK {expected_major}+",
                    j.version_label(),
                    j.home
                ),
                CheckStatus::Pass,
            ),
            (Some((v0, v1, v2)), None) => {
                let mut it = item(
                    "java",
                    "Java 运行时",
                    "运行时",
                    &format!("JDK {expected_major}+"),
                    &format!(
                        "安装版 JDK {}.{}.{}（环境变量 PATH，版本过低）；本机未安装其他 JDK {expected_major}+ 运行时",
                        v0, v1, v2
                    ),
                    CheckStatus::Fail,
                );
                it.fix_hint = format!(
                    "可通过「一键修复」自动安装 Eclipse Temurin {expected_major} JRE（MSI 静默安装并配置 JAVA_HOME/PATH，需管理员确认）"
                );
                it
            }
            (None, None) => {
                let mut it = item(
                    "java",
                    "Java 运行时",
                    "运行时",
                    &format!("JDK {expected_major}+"),
                    "未检测到 java；本机未安装 JDK {expected_major}+ 运行时",
                    CheckStatus::Fail,
                );
                it.fix_hint = format!(
                    "可通过「一键修复」自动安装 Eclipse Temurin {expected_major} JRE（MSI 静默安装并配置 JAVA_HOME/PATH，需管理员确认）"
                );
                it
            }
        };
        it
    };
    items.push(it);
}

/// 探测 PATH 上 java 的版本。修复决策链的第一事实：服务由 `run_cmd` 启动，
/// 用的就是 PATH 上的 java，达标与否以它为准。
pub fn probe_java_version() -> Option<(u32, u32, u32)> {
    probe("java", &["-version"]).and_then(|out| java_version_tuple(&out))
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
/// `cache`：全量检查共享的探测会话缓存；单服务检查传 None。
pub fn check_service_env_with(
    def: &ServiceDef,
    states: &DepStates,
    cache: Option<&ProbeCache>,
) -> ServiceEnvReport {
    let mut items: Vec<CheckItem> = Vec::new();
    let running = states.get(&def.id).map(|s| s.running).unwrap_or(false);

    // ===== 1. 运行时检查（kind 特有） =====
    match def.kind {
        ServiceKind::Vertx => {
            // Java 运行时：只按环境变量口径检测（安装版 JDK 是唯一支持的形态）——
            // PATH java 探测 → 注册表实时盘点（JAVA_HOME/PATH）。不再认交付包内置
            // JRE，检测与启动占位符同口径。
            let expected_major = def.java_major.unwrap_or(8);
            push_path_java_check(&mut items, expected_major, cache);
        }
        ServiceKind::VueSpa | ServiceKind::AngularSpa => {
            // 前端不预设部署形态：未运行时检查可行运行路径（nginx 或 Node 任一可用）；
            // 运行中则事实已回答，交给依赖检查（如 dfy-ui → dfy-backend）。
            if !running {
                let nginx = probe_cached(cache, "nginx", &["-v"]);
                let node = probe_cached(cache, "node", &["-v"]);
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
            // 纯远程形态（remoteOnly，Redis）：检测 = 托管端点 TCP 可达；不可达时
            // 明确引导用户去「Redis 管理 → 连接配置」完成设置（不再提示本地安装）。
            if let Some((host, port)) = states.get(&def.id).and_then(|s| s.remote.as_ref()) {
                let ok = health::check_tcp(host, *port, 800);
                let ep = format!("{host}:{port}");
                let actual = if ok {
                    format!("远程端点 {ep} 可达（TCP 连通）")
                } else {
                    format!("远程端点 {ep} 不可达——请先在「运维中心 → Redis 管理 → 连接配置」完成设置")
                };
                items.push(item(
                    &format!("service-{}", def.id),
                    "服务状态",
                    "外部依赖",
                    &format!("远程端点 {ep} 可达"),
                    &actual,
                    if ok {
                        CheckStatus::Pass
                    } else {
                        CheckStatus::Fail
                    },
                ));
            } else if running {
                items.push(item(
                    "service-state",
                    "服务状态",
                    "外部依赖",
                    "已安装并处于运行状态",
                    "服务运行中（进程/端口检测）",
                    CheckStatus::Pass,
                ));
            } else {
                let mut it = item(
                    &format!("service-{}", def.id),
                    "服务状态",
                    "外部依赖",
                    "已安装并处于运行状态",
                    "未检测到运行",
                    CheckStatus::Warn,
                );
                // 手动安装过的机器（服务已注册/目录存在）建议是「启动」，不是重装——
                // 引导去装第二份 MySQL 反而制造冲突
                if def.id == "mysql" {
                    if let Some(src) = detect_mysql_install() {
                        it.fix_hint = format!(
                            "本机已安装 MySQL（{src}），但服务未运行——请在 Windows 服务管理器启动 MySQL 服务，或到「安装与维护」页签操作后重新检查"
                        );
                    }
                }
                items.push(it);
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
                // 纯远程依赖（remoteOnly，Redis）：状态即托管端点可达性，
                // 提示引导到连接配置而非本地启动/安装。
                if let Some((host, port)) = &dep.remote {
                    let ep = format!("{host}:{port}");
                    let (status, actual) = if dep.running && dep.healthy {
                        (
                            CheckStatus::Pass,
                            format!("远程端点 {ep} 可达（TCP 连通）"),
                        )
                    } else {
                        (
                            CheckStatus::Fail,
                            format!("远程端点 {ep} 不可达——请先在「运维中心 → Redis 管理 → 连接配置」完成设置"),
                        )
                    };
                    item(
                        &format!("dep-{dep_id}"),
                        &format!("依赖：{}", dep.name),
                        "外部依赖",
                        &format!("远程端点 {ep} 可达"),
                        &actual,
                        status,
                    )
                } else {
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
                        remote: None,
                    },
                )
            })
            .collect()
    }

    /// 带托管远程端点的状态视图（remoteOnly 服务专用）。
    fn states_with_remote(
        list: &[(&str, bool, bool, &[u16], Option<(&str, u16)>)],
    ) -> DepStates {
        list.iter()
            .map(|(id, running, healthy, ports, remote)| {
                (
                    id.to_string(),
                    DepState {
                        name: id.to_string(),
                        running: *running,
                        healthy: *healthy,
                        ports: ports.to_vec(),
                        remote: remote.map(|(h, p)| (h.to_string(), p)),
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
            ("gateway-console", true, true, &[]),
            ("biosphere-spatial", true, true, &[]),
            ("dfy-backend", true, true, &[48085]),
            ("mysql", true, true, &[3306]),
            ("redis", true, true, &[6379]),
        ]);
        for id in ["dfy-ui", "gateway-console", "biosphere-spatial"] {
            let def = def_of(id);
            let report = check_service_env_with(&def, &st, None);
            assert_eq!(
                report.overall,
                CheckStatus::Pass,
                "运行中的 {id}（依赖齐全）环境检查不应告警: {:?}",
                report.items
            );
        }
    }

    /// dfy-backend 声明了 JAR 产物检查与 JDK 8 期望（pom.xml java.version=1.8）。
    /// jar 项按 requires 路径探测：内置 install_dir 是 Windows 部署路径，测试机上
    /// 必然找不到 → 该项 Warn（带修复提示），但其余项（Java/依赖）必须全 Pass。
    /// （Java 检测走环境变量口径，dev/CI 机的 java 均 >= 8。）
    #[test]
    fn dfy_backend_checks_jar_artifact_and_java8() {
        let def = def_of("dfy-backend");
        assert_eq!(def.java_major, Some(8), "dfy-backend 按 JDK 8 编译，期望值必须声明为 8");
        assert!(def.requires.iter().any(|r| r.check_id() == "jar"), "必须声明 JAR 产物检查");
        assert!(
            def.run_cmd.contains("{{java}}") || def.run_cmd.contains("{java}"),
            "run_cmd 必须用 java 占位符（检测与启动同源）"
        );

        let st = states(&[
            ("dfy-backend", false, false, &[]),
            ("mysql", true, true, &[3306]),
            ("redis", true, true, &[6379]),
        ]);
        let report = check_service_env_with(&def, &st, None);

        // jar 项存在且非 Pass（产物不在内置 Windows 路径下）
        let jar = report
            .items
            .iter()
            .find(|i| i.id == "jar")
            .expect("环境检查必须包含 jar 产物项");
        assert_ne!(jar.status, CheckStatus::Pass);
        assert!(!jar.fix_hint.is_empty(), "jar 缺失必须带修复提示");

        // 除 jar 外不允许其他非 Pass（Java 版本、依赖检查都应通过）
        let others: Vec<&CheckItem> = report
            .items
            .iter()
            .filter(|i| i.id != "jar" && i.status != CheckStatus::Pass)
            .collect();
        assert!(
            others.is_empty(),
            "dfy-backend 除 jar 产物外不应有其他非 Pass 项: {:?}",
            others
        );
    }

    /// Java 检测只有环境变量一种口径：有环境变量 JDK 的机器（JAVA_HOME/PATH/
    /// 注册表盘点可达）按安装版报 Pass；都没有的机器 Fail 并提示配置/安装。
    #[test]
    fn java_check_env_only_adapts() {
        let def = def_of("dfy-backend");
        let st = states(&[("mysql", true, true, &[3306]), ("redis", true, true, &[6379])]);
        let report = check_service_env_with(&def, &st, None);
        let java = report
            .items
            .iter()
            .find(|i| i.id == "java")
            .expect("必须包含 java 检查项");
        if java.status == CheckStatus::Pass {
            assert!(
                java.actual.contains("安装版") || java.actual.contains("本机已安装"),
                "环境变量可达的机器应按安装版检测通过: {}",
                java.actual
            );
        } else {
            assert!(
                !java.actual.contains("内置"),
                "检测不得再报内置 JRE: {}",
                java.actual
            );
        }
    }

    /// {java} 占位符解析（只认环境变量）：有 JAVA_HOME 的机器解析到安装版 java
    /// 绝对路径，否则回退 PATH java；无占位符原样返回。
    #[test]
    fn java_placeholder_env_only() {
        let resolved = crate::ops::process::resolve_java_placeholder("{java} -jar x.jar");
        let exe = if cfg!(target_os = "windows") { "java.exe" } else { "java" };
        let has_env_java = std::env::var("JAVA_HOME")
            .map(|h| {
                let bin = std::path::Path::new(&h)
                    .join("bin")
                    .join(exe);
                bin.exists()
            })
            .unwrap_or(false);
        if has_env_java {
            let frag = format!("bin{}{exe} ", std::path::MAIN_SEPARATOR);
            assert!(
                resolved.contains(&frag),
                "有 JAVA_HOME 的机器应解析到安装版 java: {resolved}"
            );
        } else {
            assert_eq!(
                resolved, "java -jar x.jar",
                "无 JAVA_HOME 且无预置 JRE 的机器应回退 PATH java"
            );
        }
        assert_eq!(
            crate::ops::process::resolve_java_placeholder("nginx -c x.conf"),
            "nginx -c x.conf",
            "不含占位符的命令必须原样返回"
        );
    }

    /// Java 版本低于声明期望（javaMajor）= 字节码无法加载 = Fail，修复提示指向 JDK 8。
    /// 此前提示错误地引导安装 JRE 17（与 dfy-boot-base 的 JDK 8 目标冲突）。
    #[test]
    fn java_fix_hint_matches_declared_major() {
        let hint = java_fix_hint(8);
        assert!(hint.contains('8'), "修复提示应包含声明版本 8: {hint}");
        assert!(!hint.contains("17"), "修复提示不得再引导安装 17: {hint}");
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
        let report = check_service_env_with(&def, &st, None);
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
        let report = check_service_env_with(&def, &st, None);
        let mysql = report.items.iter().find(|i| i.id == "dep-mysql").unwrap();
        assert_eq!(mysql.status, CheckStatus::Warn);
        assert_eq!(report.overall, CheckStatus::Warn);
    }

    /// 未运行的前端：不预设部署形态 —— nginx 或 Node.js 任一可用即 pass。
    #[test]
    fn stopped_frontend_env_accepts_node_or_nginx() {
        let def = def_of("gateway-console");
        let report = check_service_env_with(&def, &states(&[]), None);
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
        let report = check_service_env_with(&def, &states(&[]), None);
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
        let report = check_service_env_with(&def, &states(&[]), None);
        assert_eq!(report.overall, CheckStatus::Pass);
    }

    /// 纯远程模式（Redis）：检测 = 托管端点 TCP 可达。端点连通 → service 项与
    /// dep-redis 项均 Pass；不可达 → Fail 且修复提示引导去「Redis 管理 → 连接配置」，
    /// 绝不再提示本地安装（winget/brew）。
    #[test]
    fn redis_remote_endpoint_drives_checks() {
        let def = def_of("redis");
        assert!(def.remote_only, "Redis 必须声明为 remoteOnly（纯远程模式）");

        // 可达端点：本机起临时 TCP 监听模拟远程 Redis
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑定临时端口");
        let port = listener.local_addr().unwrap().port();
        let st = states_with_remote(&[
            ("redis", true, true, &[port], Some(("127.0.0.1", port))),
            (
                "dfy-backend",
                false,
                false,
                &[],
                None,
            ),
        ]);
        let report = check_service_env_with(&def, &st, None);
        let svc = report
            .items
            .iter()
            .find(|i| i.id == "service-redis")
            .expect("必须包含 service-redis 检查项");
        assert_eq!(svc.status, CheckStatus::Pass, "端点连通应 Pass: {:?}", svc);
        assert!(svc.actual.contains(&port.to_string()), "实际值应显示端点: {}", svc.actual);

        // dfy-backend 视角：dep-redis 走远程端点判定
        let be = def_of("dfy-backend");
        let report = check_service_env_with(&be, &st, None);
        let dep = report.items.iter().find(|i| i.id == "dep-redis").unwrap();
        assert_eq!(dep.status, CheckStatus::Pass, "远程端点连通时依赖应 Pass: {:?}", dep);

        // 不可达端点（空闲端口）：Fail + 引导去连接配置，不提示本地安装
        let free = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let down = states_with_remote(&[
            ("redis", false, false, &[6379], Some(("127.0.0.1", free))),
            ("dfy-backend", false, false, &[], None),
        ]);
        let report = check_service_env_with(&def, &down, None);
        let svc = report.items.iter().find(|i| i.id == "service-redis").unwrap();
        assert_eq!(svc.status, CheckStatus::Fail, "端点不可达应 Fail: {:?}", svc);
        assert!(
            svc.fix_hint.contains("连接配置"),
            "修复提示应引导去连接配置: {}",
            svc.fix_hint
        );
        assert!(
            !svc.fix_hint.contains("winget") && !svc.fix_hint.contains("brew"),
            "修复提示不得再引导本地安装: {}",
            svc.fix_hint
        );

        let report = check_service_env_with(&be, &down, None);
        let dep = report.items.iter().find(|i| i.id == "dep-redis").unwrap();
        assert_eq!(dep.status, CheckStatus::Fail, "远程端点不可达时依赖应 Fail");
        assert!(dep.fix_hint.contains("连接配置"), "依赖修复提示应引导去连接配置: {}", dep.fix_hint);
    }
}
