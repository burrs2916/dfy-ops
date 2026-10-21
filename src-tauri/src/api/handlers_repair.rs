//! 环境修复中心：本地资源目录扫描、通用修复任务、AI 结构化修复方案、白名单代执行。
//!
//! 修复来源降级链（与 ops/resource.rs 的模块文档一致）：
//! 1. 本地资源目录匹配包 → UAC 提权静默安装（「一键修复」）；
//! 2. 在线下载（仅 java 有 Adoptium 链路；有网时）；
//! 3. AI 结构化方案（steps 中白名单内的命令可确认后代执行，其余复制手动执行）。

use std::io::{BufRead, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::server::AppState;
use crate::ops::ai;
use crate::ops::resource;

/// 与 handlers.rs 的 blocking 语义一致：同步阻塞段移交 tokio blocking 池。
pub(crate) async fn blocking<T, F>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .expect("blocking task panicked")
}

// ===== 资源目录扫描 =====

#[derive(Debug, Deserialize)]
pub struct ResourcesQuery {
    /// 传入时扫描该目录（设置页对未保存草稿的即时预览）；缺省扫已配置目录
    #[serde(default)]
    pub dir: Option<String>,
}

/// GET /api/env/resources —— 扫描「设置 → 本地资源目录」，列出识别出的安装包。
/// 设置页预览与环境修复中心共用（目录内容用户随时在改，每次实时扫，不做缓存）。
pub async fn resources(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(q): axum::extract::Query<ResourcesQuery>,
) -> Json<resource::ResourceScan> {
    let dir = match q.dir {
        Some(d) if !d.trim().is_empty() => d,
        _ => state.app_settings.lock().unwrap().resource_dir.clone(),
    };
    let scan = blocking(move || resource::scan(&dir)).await;
    Json(scan)
}

// ===== 通用修复任务 =====

#[derive(Debug, Deserialize)]
pub struct GenericRepairInput {
    /// 检查项 id 或目标软件名（dep-mysql / service-vernemq / java / mysql …）
    pub target: String,
}

/// POST /api/env/repair/generic —— 通用修复决策链：
/// 1. 资源目录有该目标的 ZIP 免安装版（mysql/redis/java）→ 解压 → 配置 →（注册服务）；
///    java 免安装版 = 写用户级 JAVA_HOME + 平台绝对路径指向，无 MSI/注册表/UAC；
///    MySQL 官方 msi 是 Installer 束，静默执行装不上 Server，故 zip 优先；
/// 2. 有 MSI（java/vernemq/memurai 等）→ msiexec 静默安装；
/// 3. 无包且 target=java → 在线下载 Adoptium 免安装版 zip（同 1 的解压管线）；
/// 4. 无包且其他目标 → 提示配置资源目录或使用 AI 方案。
/// 任务状态复用 /api/env/repair/status（与 java 修复同一全局单例，前端轮询逻辑不变）。
pub async fn start_generic_repair(
    State(state): State<Arc<AppState>>,
    Json(input): Json<GenericRepairInput>,
) -> Json<serde_json::Value> {
    let Some(target) = resource::repair_target(&input.target) else {
        return Json(serde_json::json!({
            "ok": false,
            "error": "该检查项不支持自动安装修复，请使用 AI 方案或按修复建议手动处理"
        }));
    };
    // Redis 为纯远程部署模式：本机不安装，检测/使用全部走「运维中心 → Redis 管理 → 连接配置」
    // 托管的远程端点。防止 API 直调触发本地安装流水线。
    if target == "redis" {
        return Json(serde_json::json!({
            "ok": false,
            "error": "Redis 为远程部署模式（本机不安装）：请到「运维中心 → Redis 管理 → 连接配置」设置主机/端口/密码后重新检测"
        }));
    }
    let label = resource::target_label(target);

    let dir = state.app_settings.lock().unwrap().resource_dir.clone();
    let (zip, msi) = if dir.is_empty() {
        (None, None)
    } else {
        blocking(move || (resource::find_zip(&dir, target), resource::find_msi(&dir, target))).await
    };

    // mysql/redis/java 支持免安装版管线；java 的安装目录固定平台托管位置
    let zip_capable = matches!(target, "mysql" | "redis" | "java");
    let started = if zip_capable && zip.is_some() {
        let install_dir = if target == "java" {
            crate::ops::env_repair::JDK_MANAGED_HOME.to_string()
        } else {
            state
                .registry
                .iter()
                .find(|d| d.id == target)
                .map(|d| d.install_dir.clone())
                .unwrap_or_else(|| format!("C:\\services\\{target}"))
        };
        crate::ops::env_repair::start_zip_install(
            target,
            zip.clone().unwrap().path,
            install_dir,
            state.env_cache.clone(),
            state.repair.clone(),
        )
    } else if let Some(f) = msi.clone() {
        crate::ops::env_repair::start_resource_install(
            target,
            f.path,
            state.env_cache.clone(),
            state.repair.clone(),
        )
    } else if target == "java" && zip.is_none() && msi.is_none() {
        // 无本地包的 java 在线兜底（免安装版 zip；expected 按注册表最低声明 8）
        crate::ops::env_repair::start_jdk_install(
            crate::ops::env_repair::JDK_DEFAULT_MAJOR,
            state.env_cache.clone(),
            state.repair.clone(),
        )
    } else {
        Err(format!(
            "本地资源目录中没有 {label} 的可安装包。请将 MSI 安装包（或 MySQL/Redis/Java 的 ZIP 免安装版）放入「设置 → 本地资源目录」后重试，或使用 AI 方案"
        ))
    };

    let source = if zip.is_some() && zip_capable {
        Some("zip")
    } else if msi.is_some() {
        Some("msi")
    } else {
        None
    };
    match started {
        Ok(()) => Json(serde_json::json!({
            "ok": true,
            "action": "install",
            "message": match source {
                Some("zip") if target == "java" => format!("开始离线安装 {label}（免安装版：解压 → 写 JAVA_HOME → 版本验证，无需管理员权限），进度见下方任务状态"),
                Some("zip") => format!("开始离线安装 {label}（免安装版：解压 → 初始化 → 注册服务），全程只需一次管理员确认，进度见下方任务状态"),
                Some("msi") => format!("开始离线安装 {label}（{}）→ 管理员确认 → 验证，进度见下方任务状态", msi.as_ref().map(|f| f.file_name.as_str()).unwrap_or_default()),
                _ => format!("资源目录无 {label} 安装包，开始在线安装免安装版（下载 → 解压 → 验证）"),
            }
        })),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

// ===== 卸载 / 维护 =====

/// 平台内卸载覆盖：mysql/redis（zip 管线，服务+目录全由平台掌控）+ java
/// （MSI 注册表条目静默卸载）。其余目标建议 AI 方案或控制面板手动卸载。
pub const UNINSTALLABLE: [&str; 3] = ["mysql", "redis", "java"];

pub(crate) fn registry_install_dir(state: &AppState, target: &str) -> String {
    state
        .registry
        .iter()
        .find(|d| d.id == target)
        .map(|d| d.install_dir.clone())
        .unwrap_or_else(|| format!("C:\\services\\{target}"))
}

/// 探测一个目标的安装痕迹（服务存在 + 目录存在；java = MSI 条目或平台免安装
/// 目录；mysql 的服务检测叠加 MSI 产品条目——外部 MySQL Installer 装的也算已装）。
/// 同时返回服务运行态（运维面板展示与启停按钮状态用；java 无服务恒 false）。
async fn probe_target(state: &AppState, target: &str) -> (bool, bool, bool) {
    if target == "java" {
        let installed = blocking(|| {
            !crate::ops::env_repair::java_msi_entries().is_empty()
                || crate::ops::env_repair::managed_java_home().is_some()
        })
        .await;
        return (installed, false, false);
    }
    let Some((svc, _)) = crate::ops::env_repair::service_spec(target) else {
        return (false, false, false);
    };
    let install_dir = registry_install_dir(state, target);
    let msi = target == "mysql";
    let (svc_exists, dir_exists, msi_installed, running) = blocking(move || {
        let exists = crate::ops::env_repair::service_exists(svc);
        let running = exists && crate::ops::env_repair::service_running(svc);
        (
            exists,
            std::path::Path::new(&install_dir).is_dir(),
            msi && !crate::ops::env_repair::mysql_msi_entries().is_empty(),
            running,
        )
    })
    .await;
    (svc_exists || msi_installed, dir_exists, running)
}

/// GET /api/env/repair/uninstall/status —— 可卸载组件的安装痕迹探测，
/// 修复中心「已安装组件管理」与 AI 方案上下文共用（每次实时探，sc query 很快）。
pub async fn uninstall_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let mut targets = Vec::new();
    for t in UNINSTALLABLE {
        let (service_installed, dir_exists, service_running) = probe_target(&state, t).await;
        // java 补充实测信息：平台托管主目录 + 实跑 java -version 的版本号
        // （MSI 装的从卸载条目名提取主版本）+ 注册表核实的 JAVA_HOME 配置态，
        // 让「装好了没、什么版本、环境变量配了没」在卡片上一眼可见
        let (java_dir, java_version, java_home_ok, java_path_ok) = if t == "java" {
            blocking(|| {
                let dir = crate::ops::env_repair::managed_java_home();
                let ver = dir
                    .as_ref()
                    .and_then(|h| crate::ops::env_repair::probe_home_version(h))
                    .map(|(a, b, c)| format!("{a}.{b}.{c}"))
                    .or_else(crate::ops::env_repair::java_msi_version_label);
                let ok = crate::ops::env_repair::java_home_configured(dir.as_deref());
                // PATH 配置态只在平台托管目录存在时评估（zip 管线管的）；
                // MSI 装的 java 由 FeatureEnvironment 自动配系统 PATH，不评不会误报缺失
                let path_ok = dir
                    .as_ref()
                    .map(|d| crate::ops::env_repair::user_path_has(&format!("{d}\\bin")))
                    .unwrap_or(true);
                (dir, ver, ok, path_ok)
            })
            .await
        } else {
            (None, None, false, false)
        };
        targets.push(serde_json::json!({
            "target": t,
            "label": resource::target_label(t),
            // java：平台 zip 管线装的返回托管主目录（MSI 装的由 Windows Installer 管理，为空）
            "installDir": if t == "java" {
                java_dir.unwrap_or_default()
            } else {
                registry_install_dir(&state, t)
            },
            "version": java_version,
            "javaHomeConfigured": java_home_ok,
            "javaPathConfigured": java_path_ok,
            "serviceInstalled": service_installed,
            "dirExists": dir_exists,
            "serviceRunning": service_running,
            "canUninstall": service_installed || dir_exists,
        }));
    }
    Json(serde_json::json!({ "targets": targets }))
}

#[derive(Deserialize)]
pub struct ServiceActionReq {
    pub target: String,
    /// "start" | "stop"
    pub action: String,
}

/// POST /api/env/repair/service —— 启动 / 停止平台注册的第三方服务
/// （MySQL80、Redis）。net start/stop 需要管理员权限，走 runas_wait 一次 UAC；
/// 幂等：已运行再 start、已停止再 stop 直接成功返回。
pub async fn service_action(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ServiceActionReq>,
) -> Json<serde_json::Value> {
    if !UNINSTALLABLE.contains(&req.target.as_str()) {
        return Json(serde_json::json!({ "ok": false, "error": format!("不支持的服务：{}", req.target) }));
    }
    let Some((svc, _)) = crate::ops::env_repair::service_spec(&req.target) else {
        return Json(serde_json::json!({ "ok": false, "error": format!("未找到 { } 的服务定义", req.target) }));
    };
    let verb = match req.action.as_str() {
        "start" => "start",
        "stop" => "stop",
        other => {
            return Json(serde_json::json!({ "ok": false, "error": format!("未知操作：{other}") }))
        }
    };
    let svc = svc.to_string();
    let svc_probe = svc.clone();
    let running_now = blocking(move || crate::ops::env_repair::service_running(&svc_probe)).await;
    if running_now == (verb == "start") {
        return Json(serde_json::json!({ "ok": true, "running": running_now, "message": "已是目标状态，无需操作" }));
    }
    let args = format!("{verb} {svc}");
    let label = format!("{} 服务{}", crate::ops::resource::target_label(&req.target), if verb == "start" { "启动" } else { "停止" });
    let res = blocking(move || crate::ops::env_repair::runas_wait("net", &args)).await;
    match res {
        Ok(()) => {
            let svc2 = svc.clone();
            let running = blocking(move || crate::ops::env_repair::service_running(&svc2)).await;
            // net stop 后 STATE 短暂过渡，以复核为准；失败给出明确原因
            if running == (verb == "start") {
                state.repair.log(format!("{label} 完成"));
                Json(serde_json::json!({ "ok": true, "running": running, "message": format!("{label}成功") }))
            } else {
                Json(serde_json::json!({ "ok": false, "error": format!("{label}后状态未达预期（当前{}），可在服务管理器查看详情", if running { "运行中" } else { "已停止" }) }))
            }
        }
        Err(e) => Json(serde_json::json!({ "ok": false, "error": format!("{label}失败：{e}") })),
    }
}

#[derive(Deserialize)]
pub struct MysqlImportReq {
    /// 本机 .sql 文件绝对路径（前端原生文件选择器取得）
    pub file: String,
    /// 目标库：等价导入前先 USE。脚本未含建库/USE 语句时必填；
    /// 脚本自带时可为空（跟随脚本）
    #[serde(default)]
    pub database: Option<String>,
    /// 新建库名：先 CREATE DATABASE IF NOT EXISTS 再导入（用户选择"新建数据库"时提供）
    #[serde(rename = "newDatabase", default)]
    pub new_database: Option<String>,
    /// 遇错策略：stop（默认）遇错即停；continue 加 --force 跳过错误继续
    #[serde(rename = "onError", default)]
    pub on_error: Option<String>,
    /// 冲突处理策略（选库后检测到重名表时由前端下发）：
    /// direct 不处理；rebuild 先 DROP 脚本涉及的表再导入（幂等重建，推荐）；
    /// append 等价 --force 追加；recreate_db 删库重建（危险，前端已强确认）。
    #[serde(rename = "conflictStrategy", default)]
    pub conflict_strategy: Option<String>,
}

/// 新库名合法性：中文/字母/数字/下划线，1-64 字符。库名会拼进 CREATE DATABASE
/// 语句，必须收紧字符集（不允许反引号/分号/空白），杜绝拼接注入。
fn valid_db_name(name: &str) -> bool {
    let n = name.chars().count();
    n > 0
        && n <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || (c as u32) >= 0x4E00)
}

/// 导入进度实时视图（跨线程共享，全部原子/锁内数据）。
#[derive(Clone)]
struct ImportProgress {
    running: Arc<AtomicBool>,
    /// preparing / creating_db / clearing_tables / recreating_db / importing / done
    phase: Arc<Mutex<&'static str>>,
    bytes: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    errors: Arc<AtomicUsize>,
    /// stderr 行滚动缓冲（cap 200，丢头保尾）：结果明细与失败 tail 都取自这里
    lines: Arc<Mutex<std::collections::VecDeque<String>>>,
}

/// 导入进度槽：同屏一次只跑一个导入任务，单槽即可。
fn import_progress_slot() -> &'static Mutex<Option<ImportProgress>> {
    static SLOT: OnceLock<Mutex<Option<ImportProgress>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 进度收尾 guard：导入闭包任何 return（含提前 Err）都把槽标记为完成，
/// 防止校验失败等早退路径让前端轮询永远显示"导入中"。
struct ImportProgressGuard;
impl Drop for ImportProgressGuard {
    fn drop(&mut self) {
        if let Some(p) = import_progress_slot().lock().unwrap().as_ref() {
            p.running.store(false, Ordering::Relaxed);
            *p.phase.lock().unwrap() = "done";
        }
    }
}

/// 取 stderr 尾部有效错误信息（最多 3 行，优先保留含 ERROR 的行）。
fn stderr_tail_impl(stderr: &str) -> String {
    stderr
        .lines()
        .filter(|l| l.contains("ERROR") || !l.trim().is_empty())
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" | ")
}

/// POST /api/env/repair/mysql/import —— 导入 SQL 到本机 MySQL。
/// mysql 客户端从平台安装目录定位；文件内容经 stdin 直灌（无 shell 重定向，
/// 路径含空格/中文均安全）；凭据取平台托管（config/mysql_credential.json）。
/// 支持目标库关联（-D 等价 USE）、先建库、遇错策略。
pub async fn mysql_import(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MysqlImportReq>,
) -> Json<serde_json::Value> {
    // 凭据走平台托管（数据库运维页设置过 root 密码则带密码导入）
    let cred = crate::ops::mysql_credential::load(&state.config_dir);
    let file = req.file;
    let database = req.database.clone().unwrap_or_default();
    let new_database = req.new_database.clone().unwrap_or_default();
    let on_error = req.on_error.clone().unwrap_or_else(|| "stop".into());
    let strategy = req.conflict_strategy.clone().unwrap_or_else(|| "direct".into());
    let file_for_log = file.clone();
    let st = state.clone();
    let res = blocking(move || {
        // 客户端定位与数据库运维同口径（顶层 → 嵌套 → MSI 常见位 → PATH）
        let client = super::handlers_mysql::mysql_client(&st)?;
        if !std::path::Path::new(&file).is_file() {
            return Err(format!("SQL 文件不存在：{file}"));
        }
        // 实际导入目标：显式指定 > 新建库（建完即用）。新建库若不随同 -D 切换，
        // 脚本又没有 USE 语句，会报 "No database selected"（实测踩过）。
        let mut target_db = database.trim().to_string();
        let base_args = |cmd: &mut std::process::Command| {
            cmd.args([
                "-h",
                "127.0.0.1",
                "-P",
                &cred.port.to_string(),
                "-u",
                &cred.user,
                "--default-character-set=utf8mb4",
            ]);
            if !cred.password.is_empty() {
                cmd.arg(format!("-p{}", cred.password));
            }
        };
        let stderr_tail = |stderr: &str| -> String { stderr_tail_impl(stderr) };
        // guard：任何 return 路径都收尾进度槽（防止早退路径卡死前端轮询）
        let _progress_guard = ImportProgressGuard;
        // 进度槽初始化：total = 文件大小，前端按字节进度轮询展示
        let prog = ImportProgress {
            running: Arc::new(AtomicBool::new(true)),
            phase: Arc::new(Mutex::new("preparing")),
            bytes: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(
                std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0),
            )),
            errors: Arc::new(AtomicUsize::new(0)),
            lines: Arc::new(Mutex::new(std::collections::VecDeque::new())),
        };
        *import_progress_slot().lock().unwrap() = Some(prog.clone());
        let set_phase = |p: &'static str| {
            *prog.phase.lock().unwrap() = p;
        };
        // 可选：先建库（脚本没带建库语句、用户选择"新建数据库"时）
        if !new_database.trim().is_empty() {
            set_phase("creating_db");
            let db = new_database.trim();
            if !valid_db_name(db) {
                return Err(format!(
                    "新库名不合法：{db}（仅允许中文、字母、数字、下划线，1-64 字符）"
                ));
            }
            let sql = format!(
                "CREATE DATABASE IF NOT EXISTS `{db}` DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci"
            );
            let mut cmd = std::process::Command::new(&client);
            base_args(&mut cmd);
            cmd.args(["-e", &sql]);
            let out = cmd
                .output()
                .map_err(|e| format!("无法启动 mysql 客户端：{e}"))?;
            if !out.status.success() {
                return Err(format!("创建数据库 {db} 失败：{}", stderr_tail(&String::from_utf8_lossy(&out.stderr))));
            }
            target_db = db.to_string();
        }
        // 冲突处理策略（选库检测到重名表后用户选择的方式）
        match strategy.as_str() {
            "rebuild" => {
                set_phase("clearing_tables");
                // 幂等重建：只 DROP 脚本涉及的表（全限定名，库里其他表不动）。
                // 表名来自脚本提取，同样过白名单——不合法的跳过，导入时自然报错。
                let info = analyze_sql_head(&file)?;
                let db = target_db.clone();
                if db.is_empty() {
                    return Err("重建模式需要先选择目标数据库".into());
                }
                let names: Vec<String> = info
                    .tables
                    .iter()
                    .filter(|t| valid_db_name(t))
                    .map(|t| format!("`{db}`.`{t}`"))
                    .collect();
                if !names.is_empty() {
                    let sql = format!("DROP TABLE IF EXISTS {}", names.join(", "));
                    let mut cmd = std::process::Command::new(&client);
                    base_args(&mut cmd);
                    cmd.args(["-e", &sql]);
                    let out = cmd
                        .output()
                        .map_err(|e| format!("无法启动 mysql 客户端：{e}"))?;
                    if !out.status.success() {
                        return Err(format!(
                            "清理已存在的表失败：{}",
                            stderr_tail(&String::from_utf8_lossy(&out.stderr))
                        ));
                    }
                }
            }
            "recreate_db" => {
                set_phase("recreating_db");
                // 整库重建（前端已强确认）：DROP DATABASE → CREATE → 导入
                let db = target_db.clone();
                if db.is_empty() {
                    return Err("整库重建需要先选择目标数据库".into());
                }
                let sql = format!(
                    "DROP DATABASE IF EXISTS `{db}`; CREATE DATABASE `{db}` DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci"
                );
                let mut cmd = std::process::Command::new(&client);
                base_args(&mut cmd);
                cmd.args(["-e", &sql]);
                let out = cmd
                    .output()
                    .map_err(|e| format!("无法启动 mysql 客户端：{e}"))?;
                if !out.status.success() {
                    return Err(format!(
                        "重建数据库 {db} 失败：{}",
                        stderr_tail(&String::from_utf8_lossy(&out.stderr))
                    ));
                }
            }
            _ => {}
        }
        set_phase("importing");
        let mut cmd = std::process::Command::new(&client);
        base_args(&mut cmd);
        if !target_db.is_empty() {
            // -D 等价导入前 USE；argv 直传，无 shell 拼接风险
            cmd.args(["-D", &target_db]);
        }
        if on_error == "continue" || strategy == "append" {
            cmd.arg("--force");
        }
        // stdin 管道 + stdout 丢弃：mysql 导入无 stdout 输出，null() 防管道积压
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::null());
        let mut child = cmd.spawn().map_err(|e| format!("无法启动 mysql 客户端：{e}"))?;
        let mut child_stdin = child.stdin.take().ok_or("无法建立导入管道")?;
        let stderr_pipe = child.stderr.take();

        // 喂入线程：分块读文件写 stdin，管道写满时阻塞（背压）——字节计数
        // 因此近似等于 mysql 实际消费进度，正是进度条需要的语义
        let file_for_feed = file.clone();
        let feed_bytes = prog.bytes.clone();
        let feeder = std::thread::spawn(move || {
            let mut f = match std::fs::File::open(&file_for_feed) {
                Ok(f) => f,
                Err(_) => return,
            };
            let mut buf = vec![0u8; 256 * 1024];
            loop {
                match f.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if child_stdin.write_all(&buf[..n]).is_err() {
                            // mysql 提前退出（密码错/立即崩溃）→ 管道断裂，停止喂入
                            break;
                        }
                        feed_bytes.fetch_add(n as u64, Ordering::Relaxed);
                    }
                    Err(_) => break,
                }
            }
            drop(child_stdin); // EOF：mysql 开始收尾
        });

        // stderr 读线程：滚动缓冲（cap 200 行）+ ERROR 实时计数
        let prog_for_err = prog.clone();
        let err_reader = std::thread::spawn(move || {
            if let Some(se) = stderr_pipe {
                let reader = std::io::BufReader::new(se);
                for line in reader.lines().map_while(Result::ok) {
                    let mut buf_lines = prog_for_err.lines.lock().unwrap();
                    if buf_lines.len() >= 200 {
                        buf_lines.pop_front();
                    }
                    buf_lines.push_back(line.clone());
                    drop(buf_lines);
                    if line.contains("ERROR") {
                        prog_for_err.errors.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        });

        let status = child
            .wait()
            .map_err(|e| format!("等待 mysql 进程失败：{e}"))?;
        let _ = feeder.join();
        let _ = err_reader.join();

        // 错误明细与尾部错误取自滚动缓冲（stderr 全文不再持有）
        let (error_lines, tail_text) = {
            let buf_lines = prog.lines.lock().unwrap();
            let error_lines: Vec<String> = buf_lines
                .iter()
                .filter(|l| l.contains("ERROR"))
                .take(20)
                .cloned()
                .collect();
            let tail: Vec<String> = buf_lines
                .iter()
                .filter(|l| l.contains("ERROR") || !l.trim().is_empty())
                .rev()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            (error_lines, tail.join(" | "))
        };
        let skipped = prog.errors.load(Ordering::Relaxed) as u32;
        let message = if status.success() {
            if skipped > 0 {
                format!("导入完成（跳过 {skipped} 条错误语句）")
            } else {
                "导入完成".to_string()
            }
        } else if tail_text.is_empty() {
            format!("导入失败（退出码 {}）", status.code().unwrap_or(-1))
        } else {
            format!("导入失败：{tail_text}")
        };
        let mut result = serde_json::Map::new();
        result.insert("ok".into(), serde_json::Value::Bool(status.success()));
        result.insert("message".into(), serde_json::Value::String(message));
        result.insert("skippedCount".into(), serde_json::json!(skipped));
        result.insert(
            "errors".into(),
            serde_json::json!(error_lines),
        );
        result.insert("targetDb".into(), serde_json::Value::String(target_db));
        // 收尾：进度槽标记完成，前端停轮询
        prog.running.store(false, Ordering::Relaxed);
        *prog.phase.lock().unwrap() = "done";
        Ok(result)
    })
    .await;
    match res {
        Ok(result) => {
            state.repair.log(format!("MySQL 导入完成：{}", file_for_log));
            Json(serde_json::Value::Object(result))
        }
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

#[derive(Deserialize)]
pub struct MysqlImportAnalyzeReq {
    pub file: String,
}

#[derive(Deserialize)]
pub struct MysqlImportVerifyReq {
    pub database: String,
}

/// POST /api/env/repair/mysql/import/verify —— 导入后验证：目标库实际表清单与
/// 近似行数（information_schema 估算值）。前端与预分析的 scriptTables 对照，
/// 得出「哪些表没建出来」，让结构完整性一目了然。
pub async fn mysql_import_verify(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MysqlImportVerifyReq>,
) -> Json<serde_json::Value> {
    let cred = crate::ops::mysql_credential::load(&state.config_dir);
    let st = state.clone();
    let res = blocking(move || {
        let db = req.database.trim().to_string();
        if !valid_db_name(&db) {
            return Err(format!("库名不合法：{db}"));
        }
        let client = super::handlers_mysql::mysql_client(&st)?;
        let sql = format!(
            "SELECT table_name, table_rows FROM information_schema.tables \
             WHERE table_schema='{db}' AND table_type='BASE TABLE' ORDER BY table_name"
        );
        let mut cmd = std::process::Command::new(&client);
        cmd.args([
            "-h", "127.0.0.1",
            "-P", &cred.port.to_string(),
            "-u", &cred.user,
            "-N", "-B",
            "-e", &sql,
        ]);
        if !cred.password.is_empty() {
            cmd.arg(format!("-p{}", cred.password));
        }
        let out = cmd
            .output()
            .map_err(|e| format!("无法启动 mysql 客户端：{e}"))?;
        if !out.status.success() {
            return Err(format!(
                "验证目标库失败：{}",
                stderr_tail_impl(&String::from_utf8_lossy(&out.stderr))
            ));
        }
        // -B tab 分隔：name\trows；rows 可能是 NULL（未 ANALYZE 的表）→ 显示为空
        let tables: Vec<serde_json::Value> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let mut it = l.splitn(2, '\t');
                let name = it.next().unwrap_or("").trim().to_string();
                let rows = it.next().unwrap_or("").trim();
                serde_json::json!({
                    "name": name,
                    "rows": if rows.is_empty() || rows == "NULL" { serde_json::Value::Null } else { serde_json::json!(rows) },
                })
            })
            .collect();
        Ok(serde_json::json!({ "tables": tables }))
    })
    .await;
    match res {
        Ok(v) => {
            let mut m = serde_json::Map::new();
            m.insert("ok".into(), serde_json::Value::Bool(true));
            if let serde_json::Value::Object(rest) = v {
                m.extend(rest);
            }
            Json(serde_json::Value::Object(m))
        }
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

/// GET /api/env/repair/mysql/import/progress —— 导入进度轮询（字节/阶段/实时错误数）。
/// mysql 客户端自身无进度输出，进度由后端喂入线程按已写入管道字节数维护。
pub async fn mysql_import_progress() -> Json<serde_json::Value> {
    let slot = import_progress_slot().lock().unwrap();
    match slot.as_ref() {
        Some(p) => Json(serde_json::json!({
            "running": p.running.load(Ordering::Relaxed),
            "phase": *p.phase.lock().unwrap(),
            "bytes": p.bytes.load(Ordering::Relaxed),
            "total": p.total.load(Ordering::Relaxed),
            "errors": p.errors.load(Ordering::Relaxed),
        })),
        None => Json(
            serde_json::json!({ "running": false, "phase": "idle", "bytes": 0, "total": 0, "errors": 0 }),
        ),
    }
}

/// POST /api/env/repair/mysql/import/analyze —— 导入前轻量预分析。
/// 只读文件前 256KB（建库/USE 语句几乎总在头部），去注释后检测
/// CREATE DATABASE / USE 语句并提取库名，供前端判断是否必须选目标库。
pub async fn mysql_import_analyze(
    Json(req): Json<MysqlImportAnalyzeReq>,
) -> Json<serde_json::Value> {
    let res = blocking(move || analyze_sql_head(&req.file)).await;
    match res {
        Ok(info) => Json(serde_json::json!({
            "ok": true,
            "sizeBytes": info.size,
            "hasCreateDb": info.has_create_db,
            "createDbName": info.create_db_name,
            "useDb": info.use_db,
            "scriptTables": info.tables,
            "scriptHasDrop": info.has_drop_table,
        })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

#[derive(Deserialize)]
pub struct MysqlImportConflictsReq {
    pub file: String,
    /// 目标库（用户在向导中选定的现有库）
    pub database: String,
}

/// POST /api/env/repair/mysql/import/conflicts —— 选库后自动查冲突：
/// 一条查询同时判定「库是否存在」和「脚本表与库内现有表的重名交集」。
/// 结果驱动前端：无冲突 → 直接导入不打扰；有冲突 → 让用户选冲突处理策略。
pub async fn mysql_import_conflicts(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MysqlImportConflictsReq>,
) -> Json<serde_json::Value> {
    let cred = crate::ops::mysql_credential::load(&state.config_dir);
    let st = state.clone();
    let res = blocking(move || {
        let info = analyze_sql_head(&req.file)?;
        let db = req.database.trim().to_string();
        if !valid_db_name(&db) {
            return Err(format!("库名不合法：{db}"));
        }
        let client = super::handlers_mysql::mysql_client(&st)?;
        // -N 去表头 -B tab 分隔；两条 SELECT：第一行 Y/N（库存在），其余行 = 库内表名
        let sql = format!(
            "SELECT IF(EXISTS(SELECT 1 FROM information_schema.schemata WHERE schema_name='{db}'),'Y','N'); \
             SELECT table_name FROM information_schema.tables WHERE table_schema='{db}' AND table_type='BASE TABLE'"
        );
        let mut cmd = std::process::Command::new(&client);
        cmd.args([
            "-h", "127.0.0.1",
            "-P", &cred.port.to_string(),
            "-u", &cred.user,
            "-N", "-B",
            "-e", &sql,
        ]);
        if !cred.password.is_empty() {
            cmd.arg(format!("-p{}", cred.password));
        }
        let out = cmd
            .output()
            .map_err(|e| format!("无法启动 mysql 客户端：{e}"))?;
        if !out.status.success() {
            return Err(format!(
                "查询目标库状态失败：{}",
                String::from_utf8_lossy(&out.stderr).lines().rev().take(2).collect::<Vec<_>>().join(" | ")
            ));
        }
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let mut lines = stdout
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty());
        let db_exists = lines.next().unwrap_or("N") == "Y";
        let existing: Vec<String> = lines.map(str::to_string).collect();
        let conflicts: Vec<String> = info
            .tables
            .iter()
            .filter(|t| existing.iter().any(|e| e.eq_ignore_ascii_case(t)))
            .cloned()
            .collect();
        Ok(serde_json::json!({
            "dbExists": db_exists,
            "scriptTables": info.tables,
            "conflicts": conflicts,
            "scriptHasDrop": info.has_drop_table,
        }))
    })
    .await;
    match res {
        Ok(v) => {
            let mut m = serde_json::Map::new();
            m.insert("ok".into(), serde_json::Value::Bool(true));
            if let serde_json::Value::Object(rest) = v {
                m.extend(rest);
            }
            Json(serde_json::Value::Object(m))
        }
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

/// 去掉 SQL 注释（行注释 `--` / `#`，块注释 `/* */`），字符串/标识符内部不解析。
/// 注释里的示例语句（文档脚本常见）不应触发建库检测。
fn strip_sql_comments(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < n {
        let c = chars[i];
        if c == '-' && i + 1 < n && chars[i + 1] == '-' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '#' {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && i + 1 < n && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < n && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(n);
        } else if c == '\'' || c == '"' || c == '`' {
            // 引号串原样拷贝到配对引号（容忍未闭合），避免串内 "--" 被误判为注释
            let quote = c;
            out.push(c);
            i += 1;
            while i < n {
                out.push(chars[i]);
                if chars[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// 在语句段内提取关键字（create database / use）后的库名。段已是折叠空白版，
/// `lower` 为同长度小写版——定位用小写，取名字用原文（保留大小写）。
fn extract_db_name(segment: &str, lower: &str, keyword: &str) -> Option<String> {
    let kw_pos = lower.find(keyword)?;
    let after = &lower[kw_pos + keyword.len()..];
    let after = after.trim_start();
    let after = after
        .strip_prefix("if not exists")
        .map(str::trim_start)
        .unwrap_or(after);
    // 偏移换算到原文段（两串按 char 同步折叠，长度一致）
    let off = lower.len() - after.len();
    let orig = &segment[off.min(segment.len())..];
    let orig = orig.trim_start();
    if let Some(r) = orig.strip_prefix('`') {
        let end = r.find('`')?;
        let name = &r[..end];
        return if name.is_empty() { None } else { Some(name.to_string()) };
    }
    let name: String = orig
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || (*c as u32) >= 0x4E00)
        .collect();
    if name.is_empty() { None } else { Some(name) }
}

/// 脚本头部分析结果：建库/USE 检测 + 脚本涉及表名 + 是否自带 DROP TABLE。
#[derive(Debug, Default)]
pub struct SqlHeadInfo {
    pub size: u64,
    pub has_create_db: bool,
    pub create_db_name: Option<String>,
    pub use_db: Option<String>,
    /// CREATE TABLE 提取的表名（不含库名前缀），供冲突检测与"重建涉及的表"
    pub tables: Vec<String>,
    /// 脚本自带 DROP TABLE IF EXISTS（mysqldump 标准输出）→ 天然可重复导入
    pub has_drop_table: bool,
}

/// 分析脚本头部：建库/USE 语句、CREATE TABLE 表名、DROP TABLE 检测。
fn analyze_sql_head(path: &str) -> Result<SqlHeadInfo, String> {
    use std::io::Read;
    let meta = std::fs::metadata(path).map_err(|e| format!("无法读取 SQL 文件：{e}"))?;
    let size = meta.len();
    let mut f = std::fs::File::open(path).map_err(|e| format!("无法读取 SQL 文件：{e}"))?;
    let mut buf = vec![0u8; 256 * 1024];
    let n = f.read(&mut buf).map_err(|e| format!("无法读取 SQL 文件：{e}"))?;
    buf.truncate(n);
    let text = strip_sql_comments(&String::from_utf8_lossy(&buf));
    // 按分号切段 + 折叠空白，两版同长度（定位/取名共用偏移）
    let mut segments: Vec<(String, String)> = Vec::new();
    let mut cur = String::new();
    let mut cur_lower = String::new();
    for c in text.chars() {
        if c == ';' {
            segments.push((std::mem::take(&mut cur), std::mem::take(&mut cur_lower)));
        } else if c.is_whitespace() {
            if !cur.ends_with(' ') {
                cur.push(' ');
                cur_lower.push(' ');
            }
        } else {
            cur.push(c);
            cur_lower.push(c.to_ascii_lowercase());
        }
    }
    segments.push((cur, cur_lower));
    let mut info = SqlHeadInfo { size, ..Default::default() };
    for (seg, lower) in &segments {
        if !info.has_create_db && lower.contains("create database") {
            if let Some(name) = extract_db_name(seg, lower, "create database") {
                info.has_create_db = true;
                info.create_db_name = Some(name);
            }
        }
        if info.use_db.is_none() && lower.trim_start().starts_with("use ") {
            info.use_db = extract_db_name(seg, lower, "use");
        }
        if !info.has_drop_table && lower.contains("drop table") {
            info.has_drop_table = true;
        }
        // CREATE TABLE 表名收集（去重；提取失败的段跳过，导入时自然报错）
        if lower.contains("create table") {
            if let Some(name) = extract_table_name(seg, lower) {
                if !name.is_empty() && !info.tables.contains(&name) {
                    info.tables.push(name);
                }
            }
        }
    }
    Ok(info)
}

/// 读取一个标识符段：反引号包裹或裸标识符，返回 (内容, 剩余文本)。
fn read_identifier_part(s: &str) -> Option<(String, String)> {
    if let Some(r) = s.strip_prefix('`') {
        let end = r.find('`')?;
        return Some((r[..end].to_string(), r[end + 1..].to_string()));
    }
    let name: String = s
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || (*c as u32) >= 0x4E00)
        .collect();
    if name.is_empty() {
        None
    } else {
        Some((name.clone(), s[name.len()..].to_string()))
    }
}

/// 提取 CREATE TABLE 后的表名，容忍 `db`.`tbl` / db.tbl / `tbl` / tbl 四种形态，
/// 取最后一段作为纯表名。返回 None 表示无法解析（调用方跳过）。
fn extract_table_name(segment: &str, lower: &str) -> Option<String> {
    let kw_pos = lower.find("create table")?;
    let after = lower[kw_pos + "create table".len()..].trim_start();
    let after = after
        .strip_prefix("if not exists")
        .map(str::trim_start)
        .unwrap_or(after);
    let off = lower.len() - after.len();
    let orig = segment[off.min(segment.len())..].trim_start();
    let (mut part, mut rest) = read_identifier_part(orig)?;
    loop {
        match rest.strip_prefix('.') {
            Some(r) if r.starts_with('`') || r.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || (c as u32) >= 0x4E00) => {
                let (next, r2) = read_identifier_part(r)?;
                part = next;
                rest = r2;
            }
            _ => break,
        }
    }
    if part.is_empty() { None } else { Some(part) }
}

/// POST /api/env/repair/java-env —— 一键修复/接管 java 用户级环境变量（幂等）：
/// JAVA_HOME 指向平台托管目录 + PATH 追加 {home}\bin（注册表原始读写 + 广播）。
/// 供「已安装但 PATH/JAVA_HOME 缺失」的旧装机直接修复，无需重装。
pub async fn java_env_fix(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let res = blocking(crate::ops::env_repair::java_env_ensure).await;
    match res {
        Ok((jh, pa)) => {
            state.repair.log(format!(
                "环境变量修复完成：JAVA_HOME {}，PATH {}（请重开命令行窗口后执行 java -version 验证）",
                if jh { "已指向平台目录" } else { "本已正确，未改动" },
                if pa { "已追加 JDK bin" } else { "本已包含，未改动" }
            ));
            Json(serde_json::json!({
                "ok": true,
                "message": format!(
                    "环境变量已就绪：JAVA_HOME {}，PATH {}。请重新打开命令行窗口执行 java -version 验证",
                    if jh { "已写入" } else { "本已正确" },
                    if pa { "已加入 JDK bin" } else { "本已包含" }
                )
            }))
        }
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct UninstallInput {
    pub target: String,
    /// 同时删除安装目录（含数据库数据——不可恢复，前端弹窗默认不勾）
    #[serde(rename = "removeData", default)]
    pub remove_data: bool,
}

/// POST /api/env/repair/uninstall —— 卸载：mysql/redis 一次 UAC 完成停服务 +
/// 注销服务（removeData 时连安装目录一起删）；java 为 MSI 条目静默卸载
/// （removeData 不适用）。任务状态复用 /api/env/repair/status（同一全局单例）。
pub async fn uninstall(
    State(state): State<Arc<AppState>>,
    Json(input): Json<UninstallInput>,
) -> Json<serde_json::Value> {
    let Some(target) = resource::repair_target(&input.target) else {
        return Json(serde_json::json!({"ok": false, "error": "未知的目标组件"}));
    };
    if !UNINSTALLABLE.contains(&target) {
        return Json(serde_json::json!({"ok": false, "error": "该组件暂不支持平台内卸载（当前覆盖 MySQL/Redis/Java），请使用 AI 方案或控制面板手动卸载"}));
    }
    let install_dir = registry_install_dir(&state, target);
    let message = if target == "java" {
        "开始卸载 Java 运行时（枚举 MSI 条目 → 静默逐条卸载）→ 管理员确认 → 复核，进度见下方任务状态".to_string()
    } else {
        format!(
            "开始卸载 {}（{}数据目录）→ 管理员确认 → 复核，进度见下方任务状态",
            resource::target_label(target),
            if input.remove_data { "含" } else { "保留" }
        )
    };
    match crate::ops::env_repair::start_uninstall(
        target,
        input.remove_data,
        install_dir,
        state.config_dir.to_string_lossy().to_string(),
        state.env_cache.clone(),
        state.repair.clone(),
    ) {
        Ok(()) => Json(serde_json::json!({"ok": true, "action": "uninstall", "message": message})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

// ===== AI 结构化修复方案 =====

#[derive(Debug, Deserialize)]
pub struct PlanCheckItem {
    pub id: String,
    pub name: String,
    pub status: String,
    pub expected: String,
    pub actual: String,
    #[serde(rename = "fixHint", default)]
    pub fix_hint: String,
    #[serde(default)]
    pub category: String,
}

#[derive(Debug, Deserialize)]
pub struct RepairPlanInput {
    #[serde(rename = "checkItems")]
    pub check_items: Vec<PlanCheckItem>,
    /// 上一轮方案文本（多轮追问时携带，供模型延续上下文修正方案）
    #[serde(rename = "previousPlan", default)]
    pub previous_plan: Option<String>,
    /// 用户追问内容（多轮对话）
    #[serde(rename = "followUp", default)]
    pub follow_up: Option<String>,
}

/// 方案里的一个步骤。command 在代执行白名单内时前端弹确认框后代执行，
/// 否则只提供「复制」——AI 生成的内容不可信，绝不静默执行。
#[derive(Debug, Clone, Serialize)]
pub struct PlanStep {
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(rename = "isExecutable", default)]
    pub is_executable: bool,
}

#[derive(Debug, Serialize)]
pub struct RepairPlan {
    pub ok: bool,
    /// "ai" = 模型返回合法 JSON；"text" = 模型内容非 JSON，原文兜底展示
    pub mode: String,
    pub summary: String,
    pub steps: Vec<PlanStep>,
    pub error: Option<String>,
}

fn build_plan_prompt(
    items: &[PlanCheckItem],
    files: &[resource::ResourceFile],
    installed: &[(String, bool)],
    previous_plan: Option<&str>,
    follow_up: Option<&str>,
) -> String {
    let mut p = String::from(
        "你是 Windows 服务器运维专家。以下是运维平台环境检查发现的问题，请给出修复方案。\n\n\
         [环境检查问题]\n",
    );
    for it in items {
        p.push_str(&format!(
            "- id={} 名称={} 分类={} 状态={} 期望=[{}] 实际=[{}] 平台建议=[{}]\n",
            it.id, it.name, it.category, it.status, it.expected, it.actual, it.fix_hint
        ));
    }
    if installed.is_empty() {
        p.push_str("\n[已安装组件] 探测失败（跳过）\n");
    } else {
        p.push_str("\n[已安装组件（Windows 服务级探测）]\n");
        for (label, on) in installed {
            p.push_str(&format!(
                "- {label}：{}\n",
                if *on { "已安装（有 Windows 服务）" } else { "未安装" }
            ));
        }
    }
    if let Some(prev) = previous_plan {
        p.push_str("\n[上一轮方案]\n");
        p.push_str(prev);
        p.push_str("\n（请在上一轮方案基础上，结合用户追问调整并输出完整的新方案 JSON）\n");
    }
    if let Some(fq) = follow_up {
        p.push_str("\n[用户追问]\n");
        p.push_str(fq);
        p.push_str("\n");
    }
    if files.is_empty() {
        p.push_str("\n[本地资源目录] 未配置或没有识别出的安装包。\n");
    } else {
        p.push_str("\n[本地资源目录可用的安装包]（优先使用本地包离线安装）\n");
        for f in files {
            let target = f.target.as_deref().unwrap_or("未识别");
            p.push_str(&format!(
                "- {}（{}，{} MB，识别为 {}）\n",
                f.file_name,
                f.path,
                f.size_bytes / 1024 / 1024,
                target
            ));
        }
    }
    p.push_str(
        "\n[代执行白名单] msiexec, winget, choco, sc, net, netsh, robocopy, xcopy, curl\n\
         平台可以代执行白名单内程序的【单条】命令（执行前用户会确认）。禁止管道、重定向、多条命令拼接（&& ; | 均不可用）。\n\n\
         要求：\n\
         1. 只回复 JSON，不要 markdown 代码块或其他文字；\n\
         2. 格式：{\"summary\": \"一句话方案概述\", \"steps\": [{\"title\": \"步骤名\", \"detail\": \"具体说明\", \"command\": \"可选，白名单内单条命令\", \"needsAdmin\": true|false}]}\n\
         3. 本地资源目录里有对应安装包时，优先给出 msiexec /i \"完整路径\" /quiet 形式的命令；\n\
         4. steps 不超过 6 步，按执行顺序排列。\n",
    );
    p
}

/// 纯文本兜底：模型没给 JSON 时原文透传（不丢信息）。
fn text_fallback(text: &str) -> RepairPlan {
    RepairPlan {
        ok: true,
        mode: "text".into(),
        summary: format!("AI 未返回结构化方案，以下为原始建议：\n\n{text}"),
        steps: vec![],
        error: None,
    }
}

/// 容错解析模型输出：提取首尾大括号之间的内容按 JSON 解析；失败则原文兜底。
fn parse_plan_reply(text: &str) -> RepairPlan {
    let trimmed = text.trim();
    let start = match trimmed.find('{') {
        Some(i) => i,
        None => return text_fallback(text),
    };
    let end = match trimmed.rfind('}') {
        Some(i) if i > start => i + 1,
        _ => return text_fallback(text),
    };
    match serde_json::from_str::<serde_json::Value>(&trimmed[start..end]) {
        Ok(v) => {
            let summary = v["summary"].as_str().unwrap_or("修复方案").to_string();
            let mut steps = Vec::new();
            if let Some(arr) = v["steps"].as_array() {
                for s in arr {
                    let command = s["command"].as_str().map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
                    // isExecutable 由后端按白名单裁决，不信任模型自述
                    let is_executable =
                        command.as_deref().map(|c| resource::validate_command(c).is_ok()).unwrap_or(false);
                    steps.push(PlanStep {
                        title: s["title"].as_str().unwrap_or("未命名步骤").to_string(),
                        detail: s["detail"].as_str().unwrap_or("").to_string(),
                        command,
                        is_executable,
                    });
                }
            }
            if steps.is_empty() {
                return RepairPlan {
                    ok: true,
                    mode: "text".into(),
                    summary: format!("{summary}\n\n{text}"),
                    steps: vec![],
                    error: None,
                };
            }
            RepairPlan {
                ok: true,
                mode: "ai".into(),
                summary,
                steps,
                error: None,
            }
        }
        Err(_) => RepairPlan {
            ok: true,
            mode: "text".into(),
            summary: format!("AI 返回内容无法解析为结构化方案，以下为原始建议：\n\n{text}"),
            steps: vec![],
            error: None,
        },
    }
}

/// POST /api/env/repair/plan —— 已配置 AI 助手时生成结构化修复方案。
/// 输入当前全部问题项（fail/warn），后端附带资源目录清单做上下文。
pub async fn repair_plan(
    State(state): State<Arc<AppState>>,
    Json(input): Json<RepairPlanInput>,
) -> Json<RepairPlan> {
    if input.check_items.is_empty() {
        return Json(RepairPlan {
            ok: false,
            mode: "none".into(),
            summary: String::new(),
            steps: vec![],
            error: Some("没有需要修复的问题项".into()),
        });
    }
    let llm = state.ai_models.lock().unwrap().active();
    let Some(llm) = llm else {
        return Json(RepairPlan {
            ok: false,
            mode: "none".into(),
            summary: String::new(),
            steps: vec![],
            error: Some("尚未配置 AI 助手，请先在「AI 助手」页配置模型".into()),
        });
    };

    let dir = state.app_settings.lock().unwrap().resource_dir.clone();
    let files = blocking(move || resource::scan(&dir).files).await;
    // 已安装状态：让 AI 区分「全新安装」与「修复/重装」，方案更贴近实情
    let mut installed = Vec::new();
    for t in UNINSTALLABLE {
        let (on, _, _) = probe_target(&state, t).await;
        installed.push((resource::target_label(t).to_string(), on));
    }
    let prompt = build_plan_prompt(
        &input.check_items,
        &files,
        &installed,
        input.previous_plan.as_deref(),
        input.follow_up.as_deref(),
    );

    match ai::analyze(&llm, prompt).await {
        Ok(text) => Json(parse_plan_reply(&text)),
        Err(e) => Json(RepairPlan {
            ok: false,
            mode: "none".into(),
            summary: String::new(),
            steps: vec![],
            error: Some(format!("AI 请求失败：{e}")),
        }),
    }
}

// ===== 白名单代执行 =====

#[derive(Debug, Deserialize)]
pub struct ExecInput {
    pub command: String,
}

#[derive(Debug, Serialize)]
pub struct ExecResult {
    pub ok: bool,
    pub error: Option<String>,
}

/// POST /api/env/repair/exec —— 白名单代执行（UAC 提权，单条命令）。
///
/// 安全边界：
/// - validate_command：首 token 必须在白名单（msiexec/winget/choco/sc/net/netsh/
///   robocopy/xcopy/curl），且禁止 `; | & ` ` $` 等元字符——AI 生成的命令不可信；
/// - 通过后再把命令拆为「程序 + 参数串」交给 Start-Process -Verb RunAs，
///   参数串放 PowerShell 单引号字面量（无插值、无 shell 解释层），用户在 UAC
///   弹窗点击确认 = 授权动作。
pub async fn repair_exec(
    State(_state): State<Arc<AppState>>,
    Json(input): Json<ExecInput>,
) -> Json<ExecResult> {
    if let Err(e) = resource::validate_command(&input.command) {
        return Json(ExecResult {
            ok: false,
            error: Some(e),
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = input;
        return Json(ExecResult {
            ok: false,
            error: Some("代执行仅支持 Windows 目标机，请复制命令手动执行".into()),
        });
    }
    #[cfg(target_os = "windows")]
    {
        let cmd = input.command.trim().to_string();
        // 拆首 token（支持带引号的 "msiexec.exe"）与剩余参数（保留原始引号）
        let (exe, rest) = if let Some(stripped) = cmd.strip_prefix('"') {
            match stripped.find('"') {
                Some(i) => (
                    stripped[..i].to_string(),
                    stripped[i + 1..].trim().to_string(),
                ),
                None => (stripped.to_string(), String::new()),
            }
        } else {
            match cmd.find(char::is_whitespace) {
                Some(i) => (cmd[..i].to_string(), cmd[i..].trim().to_string()),
                None => (cmd.clone(), String::new()),
            }
        };
        let exe = exe.replace('\'', "''");
        let rest = rest.replace('\'', "''");
        let ps = if rest.is_empty() {
            format!(
                "$p = Start-Process -FilePath '{exe}' -Verb RunAs -Wait -PassThru; if ($p) {{ exit $p.ExitCode }} else {{ exit 1 }}"
            )
        } else {
            format!(
                "$p = Start-Process -FilePath '{exe}' -ArgumentList '{rest}' -Verb RunAs -Wait -PassThru; if ($p) {{ exit $p.ExitCode }} else {{ exit 1 }}"
            )
        };
        // 安装类命令可能持续数分钟（MySQL MSI / winget 下载），10 分钟硬超时兜底
        let run = tokio::time::timeout(
            std::time::Duration::from_secs(600),
            tokio::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps])
                .kill_on_drop(true)
                .output(),
        )
        .await;
        match run {
            Ok(Ok(out)) if out.status.success() => Json(ExecResult {
                ok: true,
                error: None,
            }),
            Ok(Ok(out)) => {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let msg = if stderr.is_empty() {
                    format!("命令执行失败（退出码 {:?}）", out.status.code())
                } else {
                    format!("命令执行失败：{stderr}")
                };
                Json(ExecResult {
                    ok: false,
                    error: Some(msg),
                })
            }
            Ok(Err(e)) => Json(ExecResult {
                ok: false,
                error: Some(format!("无法启动命令：{e}")),
            }),
            Err(_) => Json(ExecResult {
                ok: false,
                error: Some("命令执行超时（10 分钟），请检查目标安装器是否在等待界面输入".into()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AI 输出容错解析：干净 JSON / markdown 包裹 / 纯文本 / 缺 steps 各分支。
    #[test]
    fn parse_plan_reply_handles_llm_variants() {
        // 干净 JSON：isExecutable 由后端白名单裁决
        let plan = parse_plan_reply(
            r#"{"summary":"安装 MySQL","steps":[{"title":"静默安装","detail":"x","command":"msiexec /i \"C:/res/mysql.msi\" /quiet","needsAdmin":true},{"title":"复制执行","detail":"y","command":"powershell -c evil"}]}"#,
        );
        assert_eq!(plan.mode, "ai");
        assert_eq!(plan.steps.len(), 2);
        assert!(plan.steps[0].is_executable, "白名单内命令应可代执行");
        assert!(!plan.steps[1].is_executable, "powershell 不在白名单");

        // markdown 代码块包裹
        let wrapped = "```json\n{\"summary\":\"s\",\"steps\":[{\"title\":\"t\",\"command\":\"sc query MySQL80\"}]}\n```";
        let plan = parse_plan_reply(wrapped);
        assert_eq!(plan.mode, "ai");
        assert!(plan.steps[0].is_executable);

        // 纯文本兜底：不 panic，原文进 summary
        let plan = parse_plan_reply("请手动安装 MySQL 并启动服务。");
        assert_eq!(plan.mode, "text");
        assert!(plan.summary.contains("手动安装"));

        // 非法 JSON（截断的大括号）兜底
        let plan = parse_plan_reply("{\"summary\":\"s\", \"steps\": [");
        assert_eq!(plan.mode, "text");

        // 合法 JSON 但无 steps：原文兜底
        let plan = parse_plan_reply("{\"summary\":\"只有概述\"}");
        assert_eq!(plan.mode, "text");
        assert!(plan.summary.contains("只有概述"));
    }

    /// prompt 必须携带问题项、资源清单与白名单约定；多轮追问时带上一轮方案；
    /// 已安装状态也会并入（让 AI 区分全新安装与修复）。
    #[test]
    fn plan_prompt_contains_context() {
        let items = vec![PlanCheckItem {
            id: "dep-mysql".into(),
            name: "MySQL".into(),
            status: "fail".into(),
            expected: "3306 可达".into(),
            actual: "未监听".into(),
            fix_hint: "winget install".into(),
            category: "外部依赖".into(),
        }];
        let files = vec![resource::ResourceFile {
            file_name: "mysql-installer-community-8.0.42.0.msi".into(),
            path: "C:/res/mysql.msi".into(),
            size_bytes: 300 * 1024 * 1024,
            target: Some("mysql".into()),
            in_convention_dir: true,
        }];
        let installed = vec![
            ("MySQL".to_string(), false),
            ("Redis".to_string(), true),
        ];
        let p = build_plan_prompt(&items, &files, &installed, None, None);
        assert!(p.contains("dep-mysql") && p.contains("3306 可达"));
        assert!(p.contains("mysql-installer-community-8.0.42.0.msi"));
        assert!(p.contains("msiexec"));
        assert!(p.contains("MySQL：未安装") && p.contains("Redis：已安装"));
        let p_empty = build_plan_prompt(&items, &[], &installed, None, None);
        assert!(p_empty.contains("未配置或没有识别出的安装包"));
        let p_follow = build_plan_prompt(&items, &files, &installed, Some("上一轮：装 MySQL"), Some("端口还是不通"));
        assert!(p_follow.contains("上一轮：装 MySQL") && p_follow.contains("端口还是不通"));
    }

    /// 导入预分析：建库语句（含反引号/IF NOT EXISTS）、USE、注释里的语句不算。
    #[test]
    fn analyze_sql_head_detects_create_db_and_use() {
        let dir = std::env::temp_dir();
        let path = dir.join("dfy_ops_analyze_test.sql");
        std::fs::write(
            &path,
            "-- CREATE DATABASE `not_real`;\n\
             /* CREATE DATABASE also_fake; */\n\
             CREATE TABLE `t` (id INT);\n\
             CREATE DATABASE IF NOT EXISTS `my_shop` DEFAULT CHARACTER SET utf8mb4;\n\
             USE `my_shop`;\n\
             INSERT INTO `t` VALUES (1);",
        )
        .unwrap();
        let info = analyze_sql_head(path.to_str().unwrap()).unwrap();
        assert!(info.size > 0);
        assert!(info.has_create_db);
        assert_eq!(info.create_db_name.as_deref(), Some("my_shop"));
        assert_eq!(info.use_db.as_deref(), Some("my_shop"));
        assert!(info.tables.contains(&"t".to_string()), "应提取脚本表名: {:?}", info.tables);
        let _ = std::fs::remove_file(&path);
    }

    /// 只有建表+插入的脚本：不误报建库；USE 无反引号也能提取。
    #[test]
    fn analyze_sql_head_without_create_db() {
        let dir = std::env::temp_dir();
        let path = dir.join("dfy_ops_analyze_test2.sql");
        std::fs::write(
            &path,
            "SET NAMES utf8mb4;\nCREATE TABLE t (id INT);\nuse legacy_db;\nINSERT INTO t VALUES (1);",
        )
        .unwrap();
        let info = analyze_sql_head(path.to_str().unwrap()).unwrap();
        assert!(!info.has_create_db, "无建库语句不应误报");
        assert!(info.create_db_name.is_none());
        assert_eq!(info.use_db.as_deref(), Some("legacy_db"));
        let _ = std::fs::remove_file(&path);
    }

    /// 表名提取：`db`.`tbl` 前缀形态取纯表名；mysqldump 标准头（DROP TABLE IF EXISTS）
    /// 标记 has_drop_table——重复导入天然安全。
    #[test]
    fn analyze_sql_head_extracts_tables_and_drop_flag() {
        let dir = std::env::temp_dir();
        let path = dir.join("dfy_ops_analyze_test3.sql");
        std::fs::write(
            &path,
            "CREATE TABLE `shop`.`users` (id INT);\n\
             DROP TABLE IF EXISTS `users`;\n\
             CREATE TABLE IF NOT EXISTS `orders` (id INT);\n\
             -- CREATE TABLE `commented_out` (id INT);",
        )
        .unwrap();
        let info = analyze_sql_head(path.to_str().unwrap()).unwrap();
        assert!(info.has_drop_table);
        assert!(info.tables.contains(&"users".to_string()), "db.tbl 形态应取纯表名: {:?}", info.tables);
        assert!(info.tables.contains(&"orders".to_string()));
        assert!(!info.tables.contains(&"commented_out".to_string()), "注释里的表不应提取");
        let _ = std::fs::remove_file(&path);
    }

    /// 新库名白名单：中英文数字下划线通过，反引号/分号/空白/超长拒绝。
    #[test]
    fn valid_db_name_rejects_injection() {
        assert!(valid_db_name("my_shop"));
        assert!(valid_db_name("业务库_01"));
        assert!(!valid_db_name("a`b"));
        assert!(!valid_db_name("a;b"));
        assert!(!valid_db_name("a b"));
        assert!(!valid_db_name(""));
        assert!(!valid_db_name(&"x".repeat(65)));
    }
}
