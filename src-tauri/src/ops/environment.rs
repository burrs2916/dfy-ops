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
    /// 缺失时的修复建议（按当前平台生成，供用户在修复中心自助修复）
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
    /// 纯远程第三方（remoteOnly：Redis/VerneMQ/MinIO/GreptimeDB）的托管端点：本机无进程/端口可查，
    /// running/healthy 已是「托管端点 TCP 可达」的探测结果，端点值用于生成
    /// 面向用户的提示文案（不可达时引导去「远程部署」配置页）。
    pub remote: Option<(String, u16)>,
    /// 远程部署型服务（registry remoteOnly）：未配置端点时视为「该功能未启用」，
    /// 不进本地进程检测——否则 MinIO 等未使用的远程依赖会把整张服务卡拖成
    /// 「缺失」（2026-11-11 生产反馈：边缘业务未用 MinIO 且未配地址，
    /// edge-conductor 卡片因 dep-minio 恒红，与业务实际运行状态相悖）。
    pub remote_only: bool,
}

/// 服务 id -> 实时状态。
pub type DepStates = HashMap<String, DepState>;

/// Env 域日志 scope（与 Proc/Watchdog 同惯例：域常量定义在使用处）。
const ENV_SCOPE: &str = "Env";

/// 探测失败留痕——失败是「未安装/不满足」结论的唯一证据链。
/// 不记原因，下次出错只能猜：命令不存在？挂了？超时被杀？
/// 成功路径不记（每轮环境检查都会跑，纯噪音）。
fn probe_failed(cmd: &str, args: &[&str], reason: &str) -> Option<String> {
    crate::debug_log::entry(
        ENV_SCOPE,
        "WARN",
        "probe",
        "fail",
        &format!("cmd={cmd} {} | {reason}", args.join(" ")),
    );
    None
}

/// 读子进程输出：stdout 与 stderr 各读一遍（java/nginx -v 打 stderr，取用方按需选）。
fn read_output(child: &mut std::process::Child) -> (String, String) {
    let mut out = String::new();
    if let Some(s) = child.stdout.as_mut() {
        let _ = s.read_to_string(&mut out);
    }
    let mut err = String::new();
    if let Some(s) = child.stderr.as_mut() {
        let _ = s.read_to_string(&mut err);
    }
    (out, err)
}

/// 执行命令并取首行有效输出（兼容 java/nginx 输出到 stderr 的情况）。
///
/// 带硬超时（3s）：`java -version` 在杀软扫描/冷启动下可能显著变慢，
/// 损坏的运行时甚至可能挂起——无超时的 `output()` 会把整轮检测永久卡死。
/// 四种失败（启动失败/超时/非零退出/无输出）各自入 debug.log，reason 即证据。
fn probe(cmd: &str, args: &[&str]) -> Option<String> {
    const TIMEOUT: Duration = Duration::from_millis(3000);
    let mut child = match Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        // 命令不存在 / 无执行权限——「未安装」判定的直接证据
        Err(e) => return probe_failed(cmd, args, &format!("启动失败（命令不存在或无权限）: {e}")),
    };
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            // 超时：杀掉并视为探测失败（进程死后管道自然 EOF）
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return probe_failed(cmd, args, &format!("超过 {TIMEOUT:?} 未退出，已强制结束"));
            }
            Err(e) => return probe_failed(cmd, args, &format!("等待进程退出失败: {e}")),
        }
    };
    let (stdout, stderr) = read_output(&mut child);
    let text = if stdout.trim().is_empty() { stderr } else { stdout };
    if !status.success() {
        // 非零退出：退出码 + 输出内容是失败的直接证据（此前这里静默返回 None）
        let code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "信号终止".into());
        return probe_failed(
            cmd,
            args,
            &format!("退出码={code} 输出={}", text.trim()),
        );
    }
    match text.lines().find(|l| !l.trim().is_empty()) {
        Some(l) => Some(l.trim().to_string()),
        None => probe_failed(cmd, args, "正常退出但无任何输出"),
    }
}

/// 探测条目的缓存有效期：与全量环境报告缓存（handlers 的 ENV_CACHE_TTL=60s）
/// 同窗长——报告到期重探时探测条目恰好也到期，两者不会互相供给超期数据。
const PROBE_TTL: Duration = Duration::from_secs(60);

/// 跨请求共享的探测缓存：同一命令（java -version / nginx -v / node -v）
/// 在 TTL 内只真正执行一次子进程调用，全量/单服务检查复用结果。
///
/// 全量检查并行化后，多个 Vertx 服务会同时要 `java -version`——不共享就是
/// N 次 JVM 冷启动（每次 0.3-1s）。锁内查、锁外探，并发重复探测天然收敛为一次。
/// AppState 持有全局实例（单服务检查不再每次冷启 java）；force 全量重检入口
/// 调用 clear()——「重新检查」语义 = 彻底新探，装完 JDK 后的复检不允许
/// 被 60s 内的旧探测结果（「未安装」）污染。
#[derive(Default)]
pub struct ProbeCache {
    map: Mutex<HashMap<String, (Instant, Option<String>)>>,
}

impl ProbeCache {
    fn probe(&self, cmd: &str, args: &[&str]) -> Option<String> {
        let key = format!("{cmd}?{}", args.join(" "));
        if let Some((at, v)) = self.map.lock().unwrap().get(&key) {
            if at.elapsed() < PROBE_TTL {
                return v.clone();
            }
        }
        let v = probe(cmd, args);
        self.map.lock().unwrap().insert(key, (Instant::now(), v.clone()));
        v
    }

    /// 清空全部探测缓存（force 全量重检时调用：立即重探，不受剩余 TTL 影响）。
    pub fn clear(&self) {
        self.map.lock().unwrap().clear();
    }
}

/// 会话缓存的探测入口：cache=None 时直接探测（测试等无缓存上下文的调用方）。
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
/// 其他平台用 Temurin（Adoptium）。2026-11-03 定案：平台标准 JDK 17（向下兼容
/// 跑 JDK8 字节码），提示统一引导 JDK 17——不再按 JDK 8 引导。
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
        "java" => java_fix_hint(17),
        "runtime-path" => by_platform(
            "托管部署：winget install -e --id nginx.nginx；dev 运行：从 https://nodejs.org 安装 Node.js",
            "托管部署：brew install nginx；dev 运行：brew install node",
        ),
        "greptime" | "service-greptime" | "dep-greptime" => {
            "GreptimeDB 随边缘计算捆绑启动（端点读取其 config.toml）：请到「修复中心 → 安装维护」启动边缘计算后重新检测，无需远程部署配置".into()
        }
        "webview2" => "在目标 Windows 主机安装 Microsoft Edge WebView2 Runtime：https://developer.microsoft.com/microsoft-edge/webview2/".into(),
        "service-vernemq" | "dep-vernemq" => "VerneMQ 为远程部署模式（本机不安装）：请到「远程部署」配置虚拟机/远程主机地址后重新检测".into(),
        "service-minio" | "dep-minio" => "MinIO 为远程部署模式（本机不安装）：请到「远程部署」配置虚拟机/远程主机地址后重新检测".into(),
        "service-mysql" | "dep-mysql" => by_platform(
            "winget install -e --id Oracle.MySQL   # 或访问 https://dev.mysql.com/downloads/ 安装 MySQL Server",
            "brew install mysql   # 或用 OrbStack/Docker 容器提供",
        ),
        "service-redis" | "dep-redis" => {
            "Redis 为远程部署模式（本机不安装）：请到「远程部署」配置页检查主机/端口/密码设置后重新检测".into()
        }
        "installed" => {
            "到「修复中心 → 安装维护」待安装列表安装该应用（安装包放在资源目录约定文件夹）".into()
        }
        "dep-dfy-backend" => "在服务总览启动 DFY 后端（受管），或确认其已在外部运行".into(),
        _ => "请在修复中心根据上方“实际”信息排查并安装对应依赖".into(),
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
                if let Ok(k) = RegKey::predef(hive).open_subkey_with_flags(format!(r"{sub}\{GUID}"), flags) {
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
/// 检查模型（全部声明驱动、零硬编码端口/路径）：
/// 1. **运行时检查**（kind 特有）：Java/WebView2/前端运行方式等；
/// 2. **服务状态**（所有服务统一）：进程是事实来源 + 端口就绪分级——
///    有就是有、没有就是没有，未运行/端口未就绪必须如实出项并给出路；
/// 3. **requires 声明检查**（`ServiceDef.requires`）：路径/端口由配置声明；
/// 4. **dependsOn 依赖检查**：直接引用被依赖服务的实时状态（同一份「进程是事实来源」
///    检测结果），依赖未运行即 Fail（启动它没有意义）、运行但端口不可达即 Warn。
///
/// `states`：全部服务的实时状态视图（调用方从 to_status 构建）——
/// 进程/端口/端点结论全部复用该视图的既有实测，不在检查内重复探测。
/// `cache`：跨请求共享的探测缓存（java -version 等子进程调用，60s TTL）。
pub fn check_service_env_with(
    def: &ServiceDef,
    states: &DepStates,
    cache: Option<&ProbeCache>,
) -> ServiceEnvReport {
    let mut items: Vec<CheckItem> = Vec::new();
    let st = states.get(&def.id);
    let running = st.map(|s| s.running).unwrap_or(false);

    // ===== 1. 运行时检查（kind 特有） =====
    match def.kind {
        ServiceKind::Vertx => {
            // Java 运行时：只按环境变量口径检测（安装版 JDK 是唯一支持的形态）——
            // PATH java 探测 → 注册表实时盘点（JAVA_HOME/PATH）。不再认交付包内置
            // JRE，检测与启动占位符同口径。
            let expected_major = def.java_major.unwrap_or(17);
            push_path_java_check(&mut items, expected_major, cache);
        }
        ServiceKind::VueSpa | ServiceKind::AngularSpa => {
            // 前端不预设部署形态：未运行时检查可行运行路径（nginx 或 Node 任一可用）；
            // 运行中则事实已回答，交给依赖检查（如 dfy-ui → dfy-backend）。
            // static_site（dfy-ui）无独立运行方式——宿主
            // （nginx）要求已由 depends_on 依赖检查表达，
            // 再提示「安装 nginx/node」只会误导。
            if !running && !def.static_site {
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
            // 第三方中间件（VerneMQ/MinIO/MySQL/Redis/GreptimeDB）的服务状态
            // 已上收为 match 后的统一「服务状态」段（远程端点 / 未配置 / 运行分级 /
            // 未运行四分支），此处不再单独出项。
        }
        ServiceKind::TauriApp => {
            // 「装没装」判项（productNames 声明型）：桌面应用检测的命门——此前
            // TauriApp 分支只查 WebView2、统一状态段还明确跳过 TauriApp，导致
            // 未安装时报告全绿（2026-11-13 反馈：空间数据系统没装却检测通过）。
            if !def.product_names.is_empty() {
                if running {
                    items.push(item(
                        "installed",
                        "应用安装状态",
                        "安装",
                        "已安装",
                        "进程运行中（视为已安装）",
                        CheckStatus::Pass,
                    ));
                } else {
                    let exe_names: Vec<String> = def
                        .process_names
                        .iter()
                        .map(|n| n.trim_end_matches(".exe").to_string())
                        .collect();
                    let hit = crate::ops::appdetect::is_installed(&def.product_names, &exe_names);
                    #[cfg(target_os = "windows")]
                    let (actual, status) = if hit {
                        (
                            format!("已安装（卸载注册表记录：{}）", def.product_names.join(" / ")),
                            CheckStatus::Pass,
                        )
                    } else {
                        (
                            "卸载注册表无该应用的安装记录（未安装）".to_string(),
                            CheckStatus::Fail,
                        )
                    };
                    #[cfg(not(target_os = "windows"))]
                    let (actual, status) = if hit {
                        ("检测到本机应用记录".to_string(), CheckStatus::Pass)
                    } else {
                        (
                            "本平台无安装记录探测（以 Windows 卸载注册表为准），请人工确认是否已安装".to_string(),
                            CheckStatus::Warn,
                        )
                    };
                    items.push(item(
                        "installed",
                        "应用安装状态",
                        "安装",
                        "已安装",
                        &actual,
                        status,
                    ));
                }
            }
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
        ServiceKind::ReverseProxy => {
            // nginx 反向代理：无 kind 特有运行时检查，依赖见 requires（安装管线负责放好二进制 + conf）。
        }
    }

    // ===== 2. 服务状态（所有被托管/第三方服务统一：进程是事实来源 + 端口就绪分级） =====
    // 此前只有第三方中间件出「服务状态」项：edge-conductor、dfy-backend、nginx、
    // 前端静态站「进程没在跑」时报告里连一条状态项都没有——「有就是有，没有就是没有」
    // 要求每个服务都给出如实反馈（2026-11-11 生产反馈：已启动的被报缺失、未启动的
    // 毫无提示）。TauriApp 在 kind 分支内自带状态表述（WebView2 / 进程级检测），不重复。
    if !matches!(def.kind, ServiceKind::TauriApp) {
        if let Some((host, port)) = st.and_then(|s| s.remote.as_ref()) {
            // 托管远程端点（remoteOnly / third_party）：结论复用 states 构建时的
            // TCP 实测（同一请求内刚测过，不再重复探测）。
            // GreptimeDB 是「捆绑本地化」：端点读自 edge-conductor 的 config.toml，
            // 是它自己的子进程——按远程端点措辞会误导运维（2026-11-11 生产反馈：
            // 「我们的端点是跟随边缘计算一起启动的」）。捆绑/远程分别如实措辞。
            let ok = st.map(|s| s.running && s.healthy).unwrap_or(false);
            let ep = format!("{host}:{port}");
            let bundled = def.id == "greptime";
            let (actual, expected) = if ok {
                if bundled {
                    (
                        format!("捆绑端点 {ep} 可达（随边缘计算启动）"),
                        format!("捆绑端点 {ep} 可达"),
                    )
                } else {
                    (
                        format!("远程端点 {ep} 可达（TCP 连通）"),
                        format!("远程端点 {ep} 可达"),
                    )
                }
            } else if bundled {
                (
                    format!("捆绑端点 {ep} 不可达——请到「修复中心」启动边缘计算后重新检测"),
                    format!("捆绑端点 {ep} 可达（随边缘计算启动）"),
                )
            } else {
                (
                    format!("远程端点 {ep} 不可达——请先在「远程部署」配置页检查地址与密码设置"),
                    format!("远程端点 {ep} 可达"),
                )
            };
            items.push(item(
                &format!("service-{}", def.id),
                "服务状态",
                "外部依赖",
                &expected,
                &actual,
                if ok {
                    CheckStatus::Pass
                } else {
                    CheckStatus::Fail
                },
            ));
        } else if def.remote_only {
            // 纯远程服务从未配置端点 = 该功能未启用（与 dependsOn 段同一口径）：
            // 本机必然没有进程/端口可查，如实说明「未配置」，不误报「未运行/缺失」
            items.push(item(
                &format!("service-{}", def.id),
                "服务状态",
                "外部依赖",
                "远程端点可达，或该功能明确未启用",
                "未配置远程端点（视为未启用该功能，不影响使用）",
                CheckStatus::Pass,
            ));
        } else if running {
            // 进程在 ≠ 一定可用：端口就绪才是「能用」的证据。就绪结论复用 states
            // 的健康实测（进程反查端口 / 配置端口 TCP，同一请求内刚测过，零重复探测），
            // 通 = Pass、不通 = Warn——如实反馈「启动中或异常」并给出重检/查日志的路。
            let healthy = st.map(|s| s.healthy).unwrap_or(false);
            // edge-conductor 端口可在配置中心修改：展示与判定跟随 config.toml 实际值
            //（与 to_status / managed_healthy 同源，改过端口后不误报未就绪）
            let ports = if def.id == "edge-conductor" {
                crate::ops::edge_config::effective_ports(
                    std::path::Path::new(&def.install_dir),
                    &def.ports,
                )
            } else {
                st.map(|s| s.ports.clone()).unwrap_or_default()
            };
            let ports_txt = ports
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join("/");
            let (actual, status) = if healthy {
                if ports_txt.is_empty() {
                    ("服务运行中（进程检测通过）".to_string(), CheckStatus::Pass)
                } else {
                    (
                        format!("服务运行中（端口 {ports_txt} 就绪）"),
                        CheckStatus::Pass,
                    )
                }
            } else if ports_txt.is_empty() {
                (
                    "进程在，健康检查未通过（启动中或异常——稍候重检）".to_string(),
                    CheckStatus::Warn,
                )
            } else {
                (
                    format!("进程在，端口 {ports_txt} 未就绪（启动中或异常——稍候重检；持续未就绪请到「监控中心」查看服务日志）"),
                    CheckStatus::Warn,
                )
            };
            let mut it = item(
                "service-state",
                "服务状态",
                "运行状态",
                "已安装并处于运行状态",
                &actual,
                status,
            );
            if it.status == CheckStatus::Warn {
                it.fix_hint =
                    "端口未就绪——刚启动请稍候 10-30 秒点「重新检查」；持续未就绪请到「监控中心」查看该服务日志排查启动失败原因".into();
            }
            items.push(it);
        } else {
            // 没有就是没有：如实出「未检测到运行」（Warn），并给出路——
            // 已装过的引导「启动」而非重装
            let mut it = item(
                &format!("service-{}", def.id),
                "服务状态",
                "运行状态",
                "已安装并处于运行状态",
                "未检测到运行",
                CheckStatus::Warn,
            );
            if def.id == "mysql" {
                // 手动安装过的机器（服务已注册/目录存在）建议是「启动」，不是重装——
                // 引导去装第二份 MySQL 反而制造冲突；无任何安装痕迹时保留默认安装提示
                if let Some(src) = detect_mysql_install() {
                    it.fix_hint = format!(
                        "本机已安装 MySQL（{src}），但服务未运行——请在 Windows 服务管理器启动 MySQL 服务，或到「修复中心 → 安装维护」操作后重新检查"
                    );
                }
            } else if def.static_site {
                it.fix_hint = "静态托管站点未运行——页面由宿主 nginx 直出：请到「修复中心 → 安装维护」启动 nginx 并确认部署产物就位后重新检测".into();
            } else if !def.run_cmd.is_empty() {
                it.fix_hint = format!(
                    "未运行——请到「修复中心 → 安装维护」页签启动 {}，或在服务总览卡片点击启动后重新检测",
                    def.name
                );
            }
            items.push(it);
        }
    }

    // ===== 3. requires 声明检查（配置驱动，零硬编码） =====
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

    // ===== 4. dependsOn 依赖检查（引用被依赖服务的实时状态，同一事实来源） =====
    for dep_id in &def.depends_on {
        let it = match states.get(dep_id) {
            Some(dep) => {
                // 纯远程依赖（remoteOnly：Redis/VerneMQ/MinIO/GreptimeDB）：状态即托管端点可达性，
                // 提示引导到「远程部署」配置页而非本地启动/安装。
                if dep.remote_only && dep.remote.is_none() {
                    // 未配置端点 = 该功能未启用：如实判绿，不判缺失——
                    // 本机没有也不会有这个进程，「未运行」的 Fail 提示全是噪音
                    item(
                        &format!("dep-{dep_id}"),
                        &format!("依赖：{}", dep.name),
                        "外部依赖",
                        "远程端点可达，或该功能明确未启用",
                        "未配置远程端点（视为未启用该功能，不影响本服务）",
                        CheckStatus::Pass,
                    )
                } else if let Some((host, port)) = &dep.remote {
                    let ep = format!("{host}:{port}");
                    let (status, detail) = if dep.running && dep.healthy {
                        (
                            CheckStatus::Pass,
                            format!("远程端点 {ep} 可达（TCP 连通）"),
                        )
                    } else {
                        (
                            CheckStatus::Fail,
                            format!("远程端点 {ep} 不可达——请先在「远程部署」配置页检查地址与密码设置"),
                        )
                    };
                    item(
                        &format!("dep-{dep_id}"),
                        &format!("依赖：{}", dep.name),
                        "外部依赖",
                        &format!("远程端点 {ep} 可达"),
                        &detail,
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
                    let (status, detail) = if dep.running && dep.healthy {
                        (CheckStatus::Pass, format!("运行中，端口 {ports_txt}"))
                    } else if dep.running {
                        // 依赖在跑但端口不可达：非致命，但功能可能受限
                        (CheckStatus::Warn, format!("运行中但端口不可达（{ports_txt}）"))
                    } else {
                        // 依赖没跑：致命——先把它拉起来，启动本服务没有意义。
                        // detail 必须给出路：只写「未运行」用户会以为本服务装坏了
                        //（实测反馈：「前端报错就是缺失」）。
                        let hint = if def.run_cmd.is_empty() {
                            // 静态站点（VueSpa）：没有自己的启动动作，页面可达性完全靠依赖
                            format!("未运行——{dep_id} 起来后页面才可达；请到「修复中心 → 安装维护」启动它")
                        } else {
                            "未运行——请到「修复中心 → 安装维护」页签启动，或在本服务详情页重新检查".to_string()
                        };
                        (CheckStatus::Fail, hint)
                    };
                    item(
                        &format!("dep-{dep_id}"),
                        &format!("依赖：{}", dep.name),
                        "外部依赖",
                        "依赖服务运行且健康",
                        &detail,
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
                        remote_only: false,
                    },
                )
            })
            .collect()
    }

    /// 远程端点服务的状态快照元组：(id, running, healthy, ports, remote_host_port)
    type RemoteStatus<'a> = (&'a str, bool, bool, &'a [u16], Option<(&'a str, u16)>);

    /// 带托管远程端点的状态视图（remoteOnly 服务专用）。
    fn states_with_remote(list: &[RemoteStatus<'_>]) -> DepStates {
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
                        remote_only: true,
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
        // dfy-ui 依赖 nginx + dfy-backend，静态目录由 nginx 统一托管
        let st = states(&[
            ("dfy-ui", true, true, &[]),
            ("edge-conductor", true, true, &[]),
            ("dfy-spatial", true, true, &[]),
            ("dfy-backend", true, true, &[48085]),
            ("nginx", true, true, &[80]),
            ("mysql", true, true, &[3306]),
            ("redis", true, true, &[6379]),
        ]);
        // dfy-spatial 无 requires 也无 depends_on → 依赖齐全即 Pass。
        {
            let def = def_of("dfy-spatial");
            let report = check_service_env_with(&def, &st, None);
            assert_eq!(
                report.overall,
                CheckStatus::Pass,
                "运行中的 dfy-spatial（依赖齐全）环境检查不应告警: {:?}",
                report.items
            );
        }
        // dfy-ui 有 requires path 检查（index.html），macOS 测试机上找不到 → Warn 可接受；
        // 但依赖项必须全 Pass（dfy-backend + nginx 都在跑且健康）
        let def = def_of("dfy-ui");
        let report = check_service_env_with(&def, &st, None);
        let dep_items: Vec<_> = report.items.iter().filter(|i| i.id.starts_with("dep-")).collect();
        for item in &dep_items {
            assert_eq!(
                item.status,
                CheckStatus::Pass,
                "dfy-ui 依赖项 {} 应 Pass: {:?}",
                item.name,
                item.actual
            );
        }
    }

    /// dfy-backend 声明了 JAR 产物检查与 JDK 17 期望（2026-11-03 平台标准定案）。
    /// jar 项按 requires 路径探测：内置 install_dir 是 Windows 部署路径，测试机上
    /// 必然找不到 → 该项 Warn（带修复提示），但其余项（Java/依赖）必须全 Pass。
    /// （Java 检测走环境变量口径，dev/CI 机的 java 均 >= 8。）
    #[test]
    fn dfy_backend_checks_jar_artifact_and_java8() {
        let def = def_of("dfy-backend");
        assert_eq!(def.java_major, Some(17), "平台标准 JDK 17，期望值必须声明为 17");
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

        // 未运行必须如实出「服务状态」项并引导启动（有就是有，没有就是没有）
        let svc = report
            .items
            .iter()
            .find(|i| i.id == "service-dfy-backend")
            .expect("未运行的服务必须出状态项");
        assert_eq!(svc.status, CheckStatus::Warn);
        assert!(svc.fix_hint.contains("启动"), "未运行应引导启动: {}", svc.fix_hint);
        // 除 jar 与「服务状态」外不允许其他非 Pass（Java 版本、依赖检查都应通过）
        let others: Vec<&CheckItem> = report
            .items
            .iter()
            .filter(|i| i.id != "jar" && !i.id.starts_with("service-") && i.status != CheckStatus::Pass)
            .collect();
        assert!(
            others.is_empty(),
            "dfy-backend 除 jar 产物/服务状态外不应有其他非 Pass 项: {:?}",
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

    /// {java} 占位符解析（2026-11-03 定案托管唯一）：有平台托管 JDK → 其
    /// bin\java.exe 绝对路径；没有 → 回退裸 `java`。不再读 PATH/JAVA_HOME
    /// （系统预装 JDK 兜底已废除）。无占位符原样返回。
    #[test]
    fn java_placeholder_env_only() {
        let resolved = crate::ops::process::resolve_java_placeholder("{java} -jar x.jar");
        // 托管唯一口径（2026-11-03）：有平台托管 JDK → 解析到其 bin\java.exe
        // 绝对路径；没有（dev/CI 机）→ 回退裸 `java`。不再读 PATH/JAVA_HOME。
        match crate::ops::env_repair::managed_java_home() {
            Some(home) => {
                let exe = if cfg!(target_os = "windows") { "java.exe" } else { "java" };
                assert!(
                    resolved.starts_with(&home) && resolved.contains(exe),
                    "有托管 JDK 应解析到其 bin 下的 java: {resolved}"
                );
            }
            None => {
                assert_eq!(
                    resolved, "java -jar x.jar",
                    "无托管 JDK 应回退裸 PATH java"
                );
            }
        }
        assert_eq!(
            crate::ops::process::resolve_java_placeholder("nginx -c x.conf"),
            "nginx -c x.conf",
            "不含占位符的命令必须原样返回"
        );
    }

    /// Java 版本低于声明期望（javaMajor）= Fail，修复提示统一引导 JDK 17
    /// （2026-11-03 平台标准定案：向下兼容跑 JDK8 字节码，不再按 8 引导）。
    #[test]
    fn java_fix_hint_matches_declared_major() {
        let hint = java_fix_hint(17);
        assert!(hint.contains('1'), "修复提示应包含声明版本 17: {hint}");
        assert!(hint.contains("17"), "修复提示应引导安装 17: {hint}");
        assert!(!hint.contains("JDK 8 "), "修复提示不得再引导 JDK 8: {hint}");
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

    /// remoteOnly 依赖未配置端点 = 功能未启用 → 如实判绿（2026-11-11 生产反馈：
    /// 边缘业务未用 MinIO 且未配地址，本机无进程可探——dep-minio 恒红把整张
    /// edge-conductor 卡拖成「缺失」，与业务实际运行状态相悖）。
    /// 配置了端点则保持严格：不可达仍 Fail，不放水。
    #[test]
    fn remote_only_dep_unconfigured_is_not_missing() {
        let def = def_of("edge-conductor");
        assert!(
            def.depends_on.contains(&"minio".to_string()),
            "edge 应声明 minio 依赖（远程部署型）"
        );

        // minio 未配置端点（remote_only=true, remote=None）→ Pass·视为未启用
        let mut st = states(&[
            ("edge-conductor", true, true, &[8084]),
            ("dfy-backend", true, true, &[48085]),
            ("vernemq", true, true, &[18830]),
            ("minio", false, false, &[]),
        ]);
        st.get_mut("minio").unwrap().remote_only = true;
        let report = check_service_env_with(&def, &st, None);
        let minio = report.items.iter().find(|i| i.id == "dep-minio").unwrap();
        assert_eq!(
            minio.status,
            CheckStatus::Pass,
            "未配置的远程依赖应判绿（视为未启用）: {}",
            minio.actual
        );
        assert!(minio.actual.contains("未配置远程端点"), "actual: {}", minio.actual);
        // overall 允许其他项（如 requires 路径在测试机不存在）为 Warn，但绝不能 Fail
        assert_ne!(
            report.overall,
            CheckStatus::Fail,
            "edge 卡片不应被未配置依赖拖成缺失"
        );

        // 配置了端点但不可达 → 仍 Fail（忠于配置，不放水）
        let minio_st = st.get_mut("minio").unwrap();
        minio_st.remote = Some(("192.168.56.101".into(), 9000));
        minio_st.running = false;
        minio_st.healthy = false;
        let report2 = check_service_env_with(&def, &st, None);
        let m2 = report2.items.iter().find(|i| i.id == "dep-minio").unwrap();
        assert_eq!(m2.status, CheckStatus::Fail, "已配置端点不可达应 Fail");

        // 纯远程服务自身从未配置端点 = 未启用：出 Pass 状态项如实说明，
        // 不得报「未检测到运行」告警（本机本来就不可能有进程）
        let vernemq = def_of("vernemq");
        assert!(vernemq.remote_only, "VerneMQ 必须声明为 remoteOnly");
        let report3 = check_service_env_with(&vernemq, &DepStates::new(), None);
        let it = report3
            .items
            .iter()
            .find(|i| i.id == "service-vernemq")
            .expect("必须出服务状态项");
        assert_eq!(
            it.status,
            CheckStatus::Pass,
            "未配置远程端点应视为未启用: {}",
            it.actual
        );
        assert!(
            it.actual.contains("未配置远程端点"),
            "文案应如实说明未配置: {}",
            it.actual
        );
    }

    /// 「有就是有，没有就是没有」：edge-conductor 没在跑时必须出「服务状态」项，
    /// 且引导「启动」而非重装——此前 RustBin/Vertx/静态站等服务未运行时
    /// 报告里连一条状态项都没有（2026-11-11 生产反馈的检测盲区）。
    #[test]
    fn stopped_managed_service_reports_status_item() {
        let def = def_of("edge-conductor");
        let report = check_service_env_with(&def, &states(&[]), None);
        let it = report
            .items
            .iter()
            .find(|i| i.id == "service-edge-conductor")
            .expect("未运行的服务必须出状态项");
        assert_eq!(it.status, CheckStatus::Warn);
        assert_eq!(it.actual, "未检测到运行");
        assert!(it.fix_hint.contains("启动"), "未运行应引导启动: {}", it.fix_hint);
    }

    /// 进程在 ≠ 一定可用：端口就绪分级——就绪 = Pass，未就绪 = Warn
    ///（如实反馈「启动中或异常」并给出重检/查日志的路）。
    #[test]
    fn running_service_port_readiness_is_graded() {
        let def = def_of("edge-conductor");

        let ok = check_service_env_with(
            &def,
            &states(&[("edge-conductor", true, true, &[8084])]),
            None,
        );
        let it = ok
            .items
            .iter()
            .find(|i| i.id == "service-state")
            .expect("运行中必须出状态项");
        assert_eq!(it.status, CheckStatus::Pass);
        assert!(it.actual.contains("就绪"), "文案应反馈端口就绪: {}", it.actual);

        let down = check_service_env_with(
            &def,
            &states(&[("edge-conductor", true, false, &[8084])]),
            None,
        );
        let it = down
            .items
            .iter()
            .find(|i| i.id == "service-state")
            .expect("运行中必须出状态项");
        assert_eq!(it.status, CheckStatus::Warn, "进程在但端口未就绪必须警示");
        assert!(it.actual.contains("未就绪"), "文案应反馈端口未就绪: {}", it.actual);
        assert!(!it.fix_hint.is_empty(), "未就绪必须给出修复/排查出路");
    }

    /// GreptimeDB 是捆绑本地化：检测项措辞必须是「捆绑端点（随边缘计算启动）」，
    /// 不得再以「远程端点」误导运维（2026-11-11 生产反馈：「我们的端点是跟随
    /// 边缘计算一起启动的」）。端口选必然不可达的 1，锁死 Fail 分支的文案。
    #[test]
    fn greptime_service_state_uses_bundled_wording() {
        let def = def_of("greptime");
        let st = states_with_remote(&[("greptime", false, false, &[], Some(("127.0.0.1", 1)))]);
        let report = check_service_env_with(&def, &st, None);
        let it = report
            .items
            .iter()
            .find(|i| i.id == "service-greptime")
            .expect("greptime 应有服务状态项");
        assert_eq!(it.status, CheckStatus::Fail, "1 端口必然不可达");
        assert!(it.actual.contains("捆绑端点"), "actual: {}", it.actual);
        assert!(
            it.actual.contains("启动边缘计算"),
            "不可达时应引导启动边缘计算: {}",
            it.actual
        );
        assert!(
            !it.actual.contains("远程部署」配置页"),
            "捆绑服务不得引导去远程部署配置页: {}",
            it.actual
        );
    }

    /// 未运行的前端检测语义：
    /// - dfy-ui（static_site）：无「运行方式」检查项——
    ///   宿主要求由 dep-* 依赖检查表达，不再误导用户装 nginx/node；
    /// - 非 static 前端（未来新增）：仍按 nginx/node 任一可用判定。
    ///
    /// 探测结果通过预填 ProbeCache 注入，与测试机是否真实安装 nginx/node 解耦。
    #[test]
    fn stopped_frontend_env_accepts_node_or_nginx() {
        let def = def_of("dfy-ui");
        assert!(def.static_site, "dfy-ui 应为 static_site");

        // static_site 未运行：不产生「运行方式」检查项（宿主检查交给 depends_on）
        let report = check_service_env_with(&def, &states(&[]), None);
        assert!(
            !report.items.iter().any(|i| i.id == "runtime-path"),
            "static_site 前端不应出现 runtime-path 检查项: {:?}",
            report.items
        );
        let dep = report.items.iter().find(|i| i.id == "dep-dfy-backend");
        assert!(dep.is_some(), "dfy-ui 应依赖检查 dfy-backend: {:?}", report.items);
    }

    /// 2026-11-10 定案：gateway-console 改静态托管（Nginx :4200 直出 dist，
    /// 无 node/npm 进程）——与 dfy-ui 同口径：不得出现「运行方式（装 nginx/node）」
    /// 检查项；宿主 nginx 经 depends_on 表达；静态产物 dist 是必备检查项。
    #[test]
    fn static_gateway_console_env_skips_node_and_requires_dist() {
        let def = def_of("gateway-console");
        assert!(def.static_site, "gateway-console 应为 static_site（静态托管）");
        assert!(def.run_cmd.is_empty(), "静态托管无运行命令");
        assert!(def.depends_on.iter().any(|d| d == "nginx"), "应依赖宿主 nginx");
        assert!(def.requires.iter().any(|r| r.check_id() == "dist"), "必须声明 dist 静态产物检查");

        let report = check_service_env_with(&def, &states(&[]), None);
        assert!(
            !report.items.iter().any(|i| i.id == "runtime-path"),
            "static_site 前端不应出现 runtime-path 检查项: {:?}",
            report.items
        );
        assert!(
            report.items.iter().any(|i| i.id == "dist"),
            "环境检查必须包含 dist 静态产物项: {:?}",
            report.items
        );
    }

    /// 回归：GreptimeDB 时序库随安装管线整合到 edge-conductor/greptimedb/，
    /// 由 edgecd-daemon 自己拉起——环境检查必须有「greptimedb/greptime 就位」
    /// 路径检查项（安装完就有）；VerneMQ/MinIO 仍为远程部署，不得出现本地
    /// 路径检查项（防止「下载二进制放入本地目录」这类误导性修复建议）。
    #[test]
    fn remote_deps_have_no_local_binary_checks() {
        let def = def_of("edge-conductor");
        let req = def
            .requires
            .iter()
            .find(|r| r.check_id() == "greptime")
            .expect("edge-conductor 必须声明 greptimedb 本地路径检查项");
        assert_eq!(
            req.path.as_deref(),
            Some("greptimedb/greptime"),
            "路径必须指向随安装整合的 greptimedb 目录: {req:?}"
        );
        let vernemq = def_of("vernemq");
        let minio = def_of("minio");
        for (id, d) in [("vernemq", &vernemq), ("minio", &minio)] {
            assert!(
                d.requires.is_empty(),
                "{id} 为远程部署，不应残留本地路径检查项: {:?}",
                d.requires
            );
            let report = check_service_env_with(d, &states(&[]), None);
            assert!(
                !report
                    .items
                    .iter()
                    .any(|i| i.id == "greptime" || i.id == "vernemq" || i.id == "minio"),
                "{id} 环境检查不得出现本地路径检查项: {:?}",
                report.items
            );
        }
    }

    /// macOS：Tauri 客户端用系统内置 WKWebView，未运行不得有 WebView2 假警告；
    /// 「应用安装状态」判项在本平台无卸载注册表可查——如实给 Warn（人工确认），
    /// 不假绿（2026-11-13 反馈：未安装却全通过）也绝不误判 Fail。
    #[cfg(target_os = "macos")]
    #[test]
    fn tauri_env_passes_on_macos_without_running() {
        let def = def_of("dfy-spatial");
        let report = check_service_env_with(&def, &states(&[]), None);
        assert!(
            report.items.iter().all(|i| i.id != "webview2"),
            "macOS 不应出现 WebView2 项: {:?}",
            report.items
        );
        let installed = report
            .items
            .iter()
            .find(|i| i.id == "installed")
            .expect("dfy-spatial 声明了 productNames，必须出「应用安装状态」项");
        assert_eq!(
            installed.status,
            CheckStatus::Warn,
            "mac 无注册表可查应为 Warn（人工确认）: {installed:?}"
        );
        assert_ne!(report.overall, CheckStatus::Fail);
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

        // 不可达端点（空闲端口）：Fail + 引导去「远程部署」配置页，不提示本地安装
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
            svc.fix_hint.contains("远程部署"),
            "修复提示应引导去远程部署配置页: {}",
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
        assert!(dep.fix_hint.contains("远程部署"), "依赖修复提示应引导去远程部署配置页: {}", dep.fix_hint);
    }
}
