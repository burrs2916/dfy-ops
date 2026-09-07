use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::server::AppState;
use crate::ops::ai;
use crate::ops::diagnosis;
use crate::ops::environment;
use crate::ops::health;
use crate::ops::logs;
use crate::ops::orchestrator;
use crate::ops::process;
use crate::ops::registry::{ServiceDef, ServiceKind};

const PROBE_TIMEOUT_MS: u64 = 800;

/// 服务运行态（三态，P0-2）。
///
/// - `managed`：由本平台启动，且 `ProcManager` 持有真实存活的进程句柄
///   （P1-4 已让 `is_running` 通过 `try_wait` 真实探活并回收僵尸）。
/// - `detected`：本平台未拉起，但端口探测到在监听，或按 `processNames` 探测到
///   本机进程（无端口桌面客户端的兜底通道）——服务由外部进程拉起或历史遗留。
///   属于「在运行但不在我们掌控内」。
/// - `down`：既无受管进程、端口也未监听。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Managed,
    Detected,
    Down,
}

/// 把同步阻塞段移交 tokio 的 blocking 线程池执行。
///
/// 必须这么做的原因：Axum handler 跑在 tokio worker 线程上，worker 数默认 = CPU 核数。
/// 而本模块的 handler 内部包含大量同步阻塞操作——
///   · 端口探测（check_tcp，每端口最长 800ms，前端 5s 轮询持续触发）
///   · 环境探测（Command::output 拉起 java/nginx 子进程）
///   · 依赖编排的就绪等待（wait_ready 内 thread::sleep，单服务最长 30s）
///   · 日志文件 / 配置文件的同步读写
/// 若在 handler 里直接同步执行，一次「全部启动」就能把 worker 池占满（30s × N 个服务），
/// 期间 /api/health 与前端状态轮询全部排队，用户看到的现象就是界面卡死。
/// blocking 池默认上限 512 线程，与 worker 池隔离，因此不会互相拖累。
async fn blocking<T, F>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .expect("blocking task 执行失败（内部 panic）")
}

/// 服务的实时状态（运行 + 健康）。
#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    #[serde(rename = "kind")]
    pub kind: ServiceKind,
    pub running: bool,
    #[serde(rename = "runState")]
    pub run_state: RunState,
    pub healthy: bool,
    pub ports: Vec<u16>,
    #[serde(rename = "dependsOn")]
    pub depends_on: Vec<String>,
    #[serde(rename = "isDaemon")]
    pub is_daemon: bool,
    /// 第三方依赖（VerneMQ/MinIO 等）：平台只检测不托管，前端据此隐藏启停按钮并分组展示。
    #[serde(rename = "thirdParty")]
    pub third_party: bool,
    pub description: String,
    /// 服务的预期安装目录（Windows 路径，仅作展示与排障参考，平台不会自动创建/部署）。
    #[serde(rename = "installDir")]
    pub install_dir: String,
    /// 构建命令（若服务需要源码构建）。
    #[serde(rename = "buildCmd")]
    pub build_cmd: Option<String>,
    /// 启动命令。
    #[serde(rename = "runCmd")]
    pub run_cmd: String,
    /// 健康检查 URL（可选）。
    #[serde(rename = "healthUrl")]
    pub health_url: Option<String>,
    /// 日志文件路径列表。
    #[serde(rename = "logPaths")]
    pub log_paths: Vec<String>,
}

fn to_status(def: &ServiceDef, managed: bool) -> ServiceStatus {
    let (run_state, ports, healthy) = if managed {
        (
            RunState::Managed,
            def.ports.clone(),
            managed_healthy(def),
        )
    } else {
        detect_external(def)
    };
    ServiceStatus {
        id: def.id.clone(),
        name: def.name.clone(),
        kind: def.kind.clone(),
        running: run_state != RunState::Down,
        run_state,
        healthy,
        ports,
        depends_on: def.depends_on.clone(),
        is_daemon: def.is_daemon,
        third_party: def.third_party,
        description: def.description.clone(),
        install_dir: def.install_dir.clone(),
        build_cmd: def.build_cmd.clone(),
        run_cmd: def.run_cmd.clone(),
        health_url: def.health_url.clone(),
        log_paths: def.log_paths.clone(),
    }
}

/// 受管服务（本平台拉起）的健康判定：就绪等待本来就以配置端口为目标，沿用它。
fn managed_healthy(def: &ServiceDef) -> bool {
    if let Some(url) = &def.health_url {
        health::check_http(url, PROBE_TIMEOUT_MS)
    } else if def.ports.is_empty() {
        // 无端口的桌面客户端：是否「健康」直接等同于「在运行」
        true
    } else {
        def.ports
            .iter()
            .all(|p| health::check_tcp("127.0.0.1", *p, PROBE_TIMEOUT_MS))
    }
}

/// 外部运行服务的检测：**进程是事实来源，端口由进程反查**。
///
/// 平台不规定服务用什么端口——按 `processNames`（运行器特征，如 `ng serve`、
/// `dfy-server.jar`）定位 PID，再用 `lsof`/`netstat` 反查这些 PID 实际监听的端口：
/// 反查到什么就上报什么（前端展示与 healthy 判定均以实际端口为准）。
/// 仅当未配置进程名或未命中时，才回退到按配置端口探测（兼容 nginx 多实例这类
/// 不适合按进程名区分的部署形态）。
fn detect_external(def: &ServiceDef) -> (RunState, Vec<u16>, bool) {
    let pids: Vec<u32> = def
        .process_names
        .iter()
        .flat_map(|n| process::process_pids(n))
        .collect();
    if !pids.is_empty() {
        let discovered = process::listening_ports(&pids);
        if !discovered.is_empty() {
            // 进程在且端口在监听：以实际监听端口为准
            return (RunState::Detected, discovered, true);
        }
        // 进程在但没有监听端口：无端口预期（桌面客户端）视为健康；
        // 有端口预期则回退按配置端口确认（lsof 不可用时的兜底）。
        let healthy = if def.ports.is_empty() {
            true
        } else {
            def.ports
                .iter()
                .all(|p| health::check_tcp("127.0.0.1", *p, PROBE_TIMEOUT_MS))
        };
        return (RunState::Detected, def.ports.clone(), healthy);
    }
    // 兜底：未配置进程名或未命中 → 按配置端口探测
    let port_up = def
        .ports
        .iter()
        .any(|p| health::check_tcp("127.0.0.1", *p, PROBE_TIMEOUT_MS));
    let healthy = if let Some(url) = &def.health_url {
        health::check_http(url, PROBE_TIMEOUT_MS)
    } else if def.ports.is_empty() {
        false
    } else {
        def.ports
            .iter()
            .all(|p| health::check_tcp("127.0.0.1", *p, PROBE_TIMEOUT_MS))
    };
    let state = if port_up {
        RunState::Detected
    } else {
        RunState::Down
    };
    (state, def.ports.clone(), healthy)
}

fn find_def<'a>(state: &'a AppState, id: &str) -> Option<&'a ServiceDef> {
    state.registry.iter().find(|d| d.id == id)
}

// ===== 环境修复（java 检查项的分级修复入口） =====

#[derive(Debug, Deserialize)]
pub struct EnvRepairInput {
    #[serde(rename = "serviceId")]
    pub service_id: String,
    #[serde(rename = "checkId")]
    pub check_id: String,
}

/// POST /api/env/repair —— 分级修复决策链：
/// 1. PATH java 已达标 → action=none（无需修复）；
/// 2. 本机已有达标 JDK（PATH 不达标）→ action=configure（返回配置指引，不擅自改）；
/// 3. 纯净机无任何达标 JDK → action=install（启动自动安装：下载 Temurin MSI →
///    UAC 提权静默安装并配置环境变量 → 复探验证），前端轮询 /api/env/repair/status。
pub async fn start_env_repair(
    State(state): State<Arc<AppState>>,
    Json(input): Json<EnvRepairInput>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if input.check_id != "java" {
        return Ok(Json(serde_json::json!({
            "ok": false,
            "error": "该检查项暂不支持自动修复，请按修复建议手动处理"
        })));
    }
    let def = find_def(&state, &input.service_id)
        .ok_or(StatusCode::NOT_FOUND)?
        .clone();
    let expected = def.java_major.unwrap_or(8);

    // 绿色交付模式：声明了预置 JRE（exe 同级 resource 目录）→ 修复 = 确认目录在位，
    // 缺失则指引恢复交付包目录（免安装、离线可用），不走在线下载安装。
    if let Some(rel) = &def.java_home {
        let rel_for_task = rel.clone();
        let present =
            blocking(move || crate::ops::process::bundled_java_home(&rel_for_task).is_some()).await;
        return Ok(Json(serde_json::json!({
            "ok": true,
            "action": if present { "none" } else { "configure" },
            "message": if present {
                format!("内置 JRE 已就位（{rel}），随程序免安装交付，无需修复")
            } else {
                format!("内置 JRE 缺失：免安装部署，将交付包中的 {rel} 目录恢复到程序同级目录即可，无需安装")
            }
        })));
    }

    // 第一事实：PATH 上的 java（run_cmd 实际执行者）
    let path_major = blocking(crate::ops::environment::probe_java_version).await;
    if let Some((major, _, _)) = path_major {
        if major >= expected {
            return Ok(Json(serde_json::json!({
                "ok": true,
                "action": "none",
                "message": format!("PATH 中的 java 已满足 JDK {expected}+，无需修复")
            })));
        }
    }

    // 本机已有达标 JDK：配置问题而非缺失，给指引不代改
    let compatible = blocking(move || crate::ops::env_repair::find_compatible(expected)).await;
    if let Some(j) = compatible {
        return Ok(Json(serde_json::json!({
            "ok": true,
            "action": "configure",
            "home": j.home,
            "message": format!(
                "本机已有达标 JDK {}（{}）：无需安装，请将其 bin 目录加入 PATH 或设为 JAVA_HOME",
                j.version_label(),
                j.home
            )
        })));
    }

    // 纯净机：启动自动安装（Windows：下载+UAC 提权静默装；其他平台返回指引）
    match crate::ops::env_repair::start_jdk_install(
        expected,
        state.env_cache.clone(),
        state.repair.clone(),
    ) {
        Ok(_) => Ok(Json(serde_json::json!({
            "ok": true,
            "action": "install",
            "message": format!("开始自动安装 Eclipse Temurin {expected} JRE（下载 → 管理员确认 → 验证），进度见下方任务状态")
        }))),
        Err(e) => Ok(Json(serde_json::json!({"ok": false, "error": e}))),
    }
}

/// GET /api/env/repair/status —— 前端轮询修复任务进度。
pub async fn env_repair_status(
    State(state): State<Arc<AppState>>,
) -> Json<crate::ops::env_repair::RepairJob> {
    Json(state.repair.snapshot())
}

pub async fn server_health(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "services": state.registry.len(),
        "api": "dfy-ops local ops",
    }))
}

pub async fn list_services(State(state): State<Arc<AppState>>) -> Json<Vec<ServiceStatus>> {
    let out = blocking(move || {
        state
            .registry
            .iter()
            .map(|d| to_status(d, state.procs.is_running(&d.id)))
            .collect::<Vec<_>>()
    })
    .await;
    Json(out)
}

pub async fn get_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ServiceStatus>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?.clone();
    let running = blocking(move || state.procs.is_running(&id)).await;
    Ok(Json(to_status(&def, running)))
}

pub async fn start_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    if find_def(&state, &id).is_none() {
        return Json(serde_json::json!({"ok": false, "error": "service not found"}));
    }
    let id_for_result = id.clone();
    let results =
        blocking(move || orchestrator::start_with_deps(&state.procs, &state.registry, &id)).await;
    Json(serde_json::json!({
        "ok": results.iter().all(|r| r.ok),
        "id": id_for_result,
        "details": results,
    }))
}

pub async fn stop_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    if find_def(&state, &id).is_none() {
        return Json(serde_json::json!({"ok": false, "error": "service not found"}));
    }
    let id_for_result = id.clone();
    let r = blocking(move || state.procs.stop(&id_for_result)).await;
    Json(serde_json::json!({
        "ok": r.is_ok(),
        "id": id,
        "error": r.err().map(|e| e.to_string()),
    }))
}

pub async fn restart_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    if find_def(&state, &id).is_none() {
        return Json(serde_json::json!({"ok": false, "error": "service not found"}));
    }
    let id_for_result = id.clone();
    // stop + start 合并在同一个 blocking 任务里：stop 在 Windows 上要走 taskkill，
    // start 内含最长 30s 的就绪等待，两者都是同步阻塞，一起挪出 worker 池。
    let results = blocking(move || {
        let _ = state.procs.stop(&id);
        orchestrator::start_with_deps(&state.procs, &state.registry, &id)
    })
    .await;
    Json(serde_json::json!({
        "ok": results.iter().all(|r| r.ok),
        "id": id_for_result,
        "details": results,
    }))
}

/// 按依赖拓扑顺序启动全部常驻服务。
pub async fn start_all(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<orchestrator::ActionResult>> {
    Json(blocking(move || orchestrator::start_all(&state.procs, &state.registry)).await)
}

/// 按逆依赖顺序停止全部服务。
pub async fn stop_all(State(state): State<Arc<AppState>>) -> Json<Vec<orchestrator::ActionResult>> {
    Json(blocking(move || orchestrator::stop_all(&state.procs, &state.registry)).await)
}

pub async fn build_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?.clone();
    let Some(cmd) = def.build_cmd.clone() else {
        return Ok(Json(
            serde_json::json!({"ok": false, "error": "该服务未配置 buildCmd"}),
        ));
    };
    let log_path = PathBuf::from(&def.install_dir)
        .join("build.log")
        .to_string_lossy()
        .to_string();
    let result = blocking(move || state.procs.build(&def.install_dir, &cmd, &log_path)).await;
    Ok(Json(match result {
        Ok(_) => serde_json::json!({"ok": true, "id": id}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    }))
}

/// 读取指定服务的构建日志（前端轮询以展示构建进度）。
pub async fn build_log(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    let path = PathBuf::from(&def.install_dir).join("build.log");
    let text = blocking(move || {
        if path.exists() {
            std::fs::read_to_string(&path).unwrap_or_default()
        } else {
            String::new()
        }
    })
    .await;
    Ok(Json(serde_json::json!({"id": id, "logs": text})))
}

pub async fn service_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    let log_paths = def.log_paths.clone();
    let text = blocking(move || logs::tail_many(&log_paths, 200)).await;
    Ok(Json(serde_json::json!({"id": id, "logs": text})))
}

pub async fn diagnosis(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    // 阶段一（阻塞）：端口探测 + 子进程环境探测 + 规则诊断，整体挪到 blocking 池，
    // 避免探测耗时把 worker 线程占住（这是诊断接口里最重的一段）。
    let llm_state = state.clone();
    let (status, env_reports, findings) = blocking(move || {
        // 依赖检查视图：同一份「进程是事实来源」的状态，同时驱动服务状态与环境/依赖检查。
        let deps = dep_states(&state);
        let status: HashMap<String, (bool, bool)> = deps
            .iter()
            .map(|(id, dep)| (id.clone(), (dep.running, dep.healthy)))
            .collect();
        // 环境检查：探测各服务的运行时依赖 + requires 声明 + dependsOn 依赖，并纳入诊断。
        let mut env_reports: Vec<environment::ServiceEnvReport> = Vec::new();
        for d in &state.registry {
            env_reports.push(environment::check_service_env(d, &deps));
        }
        let findings = diagnosis::diagnose(&state.registry, &status, &env_reports);
        (status, env_reports, findings)
    })
    .await;

    // 阶段二（异步）：LLM 增强层 —— 按「模型 → 接入点 → 供应商」动态解析调用参数，
    // 未配置时退化为规则结果。网络请求本身是异步的，不占用 worker。
    let mut llm_summary: Option<String> = None;
    let mut llm_note: Option<String> = None;
    let target = llm_state.ai_models.lock().unwrap().active();
    if let Some(t) = target {
        let prompt = format!(
            "你是本地运维助手。以下是 Windows 主机上 DFY 系列服务的诊断数据，请用简体中文给出根因分析与处置建议：\n服务状态：{}\n环境检查：{}\n发现项：{}",
            serde_json::to_string(&status).unwrap_or_default(),
            serde_json::to_string(&env_reports).unwrap_or_default(),
            serde_json::to_string(&findings).unwrap_or_default()
        );
        match ai::analyze(&t, prompt).await {
            Ok(s) => llm_summary = Some(s),
            Err(e) => llm_note = Some(e),
        }
    } else {
        llm_note = Some("AI 模型未配置：请在「AI助手」添加供应商并接入点与模型".into());
    }

    Json(serde_json::json!({
        "status": status,
        "envs": env_reports,
        "findings": findings,
        "llmSummary": llm_summary,
        "llmNote": llm_note,
    }))
}

/// 构建依赖检查视图：遍历注册表，把每个服务的实时状态（进程级检测，事实来源）
/// 汇总为 DepState 映射，供环境检查的 dependsOn 段消费。
fn dep_states(state: &AppState) -> environment::DepStates {
    state
        .registry
        .iter()
        .map(|d| {
            let s = to_status(d, state.procs.is_running(&d.id));
            (
                d.id.clone(),
                environment::DepState {
                    name: d.name.clone(),
                    running: s.running,
                    healthy: s.healthy,
                    ports: d.ports.clone(),
                },
            )
        })
        .collect()
}

/// 检查单个服务的运行环境（运行时依赖 + requires 声明 + dependsOn 依赖）。
pub async fn env_check(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<environment::ServiceEnvReport>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?.clone();
    Ok(Json(
        blocking(move || {
            let deps = dep_states(&state);
            environment::check_service_env(&def, &deps)
        })
        .await,
    ))
}

/// 环境检查全量结果的缓存有效期：全量探测含多次子进程调用（java/-version 等），开销大。
const ENV_CACHE_TTL: Duration = Duration::from_secs(60);

/// 检查全部服务的运行环境（60s TTL 缓存，前端自动轮询场景下避免重复子进程探测）。
pub async fn env_check_all(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<environment::ServiceEnvReport>> {
    if let Some((at, reports)) = state.env_cache.lock().unwrap().as_ref() {
        if at.elapsed() < ENV_CACHE_TTL {
            return Json(reports.clone());
        }
    }
    let st = state.clone();
    let reports = blocking(move || {
        let deps = dep_states(&st);
        st.registry
            .iter()
            .map(|d| environment::check_service_env(d, &deps))
            .collect::<Vec<_>>()
    })
    .await;
    *state.env_cache.lock().unwrap() = Some((Instant::now(), reports.clone()));
    Json(reports)
}

// ===== 应用设置（「设置」页，与 AI 助手配置分离） =====

#[derive(Deserialize)]
pub struct AppSettingsInput {
    #[serde(rename = "statusRefreshSecs")]
    status_refresh_secs: u64,
    #[serde(rename = "envRefreshSecs")]
    env_refresh_secs: u64,
}

/// 读取应用设置。
pub async fn get_app_settings(
    State(state): State<Arc<AppState>>,
) -> Json<crate::ops::appsettings::AppSettings> {
    Json(state.app_settings.lock().unwrap().clone())
}

/// 保存应用设置到 config_dir/settings.json 并热更新内存（轮询间隔立即生效）。
pub async fn save_app_settings(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AppSettingsInput>,
) -> Json<serde_json::Value> {
    let mut s = state.app_settings.lock().unwrap().clone();
    s.status_refresh_secs = body.status_refresh_secs;
    s.env_refresh_secs = body.env_refresh_secs;
    s.sanitize();
    let config_dir = state.config_dir.clone();
    let result = blocking(move || match s.save(&config_dir) {
        Ok(_) => Ok(s),
        Err(e) => Err(e),
    })
    .await;
    match result {
        Ok(s) => {
            *state.app_settings.lock().unwrap() = s.clone();
            Json(serde_json::json!({
                "ok": true,
                "statusRefreshSecs": s.status_refresh_secs,
                "envRefreshSecs": s.env_refresh_secs,
            }))
        }
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}
