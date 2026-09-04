use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

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
use crate::ops::registry::{ServiceDef, ServiceKind};

const PROBE_TIMEOUT_MS: u64 = 800;

/// 服务的实时状态（运行 + 健康）。
#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    #[serde(rename = "kind")]
    pub kind: ServiceKind,
    pub running: bool,
    pub healthy: bool,
    pub ports: Vec<u16>,
    #[serde(rename = "dependsOn")]
    pub depends_on: Vec<String>,
    #[serde(rename = "isDaemon")]
    pub is_daemon: bool,
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

fn to_status(def: &ServiceDef, running: bool) -> ServiceStatus {
    let healthy = if let Some(url) = &def.health_url {
        health::check_http(url, PROBE_TIMEOUT_MS)
    } else if def.ports.is_empty() {
        running
    } else {
        def.ports
            .iter()
            .all(|p| health::check_tcp("127.0.0.1", *p, PROBE_TIMEOUT_MS))
    };
    ServiceStatus {
        id: def.id.clone(),
        name: def.name.clone(),
        kind: def.kind.clone(),
        running,
        healthy,
        ports: def.ports.clone(),
        depends_on: def.depends_on.clone(),
        is_daemon: def.is_daemon,
        description: def.description.clone(),
        install_dir: def.install_dir.clone(),
        build_cmd: def.build_cmd.clone(),
        run_cmd: def.run_cmd.clone(),
        health_url: def.health_url.clone(),
        log_paths: def.log_paths.clone(),
    }
}

fn find_def<'a>(state: &'a AppState, id: &str) -> Option<&'a ServiceDef> {
    state.registry.iter().find(|d| d.id == id)
}

pub async fn server_health(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "services": state.registry.len(),
        "api": "dfy-ops local ops",
    }))
}

pub async fn list_services(State(state): State<Arc<AppState>>) -> Json<Vec<ServiceStatus>> {
    let out = state
        .registry
        .iter()
        .map(|d| to_status(d, state.procs.is_running(&d.id)))
        .collect();
    Json(out)
}

pub async fn get_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ServiceStatus>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(to_status(def, state.procs.is_running(&id))))
}

pub async fn start_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    if find_def(&state, &id).is_none() {
        return Json(serde_json::json!({"ok": false, "error": "service not found"}));
    }
    let results = orchestrator::start_with_deps(&state.procs, &state.registry, &id);
    Json(serde_json::json!({
        "ok": results.iter().all(|r| r.ok),
        "id": id,
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
    let r = state.procs.stop(&id);
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
    let _ = state.procs.stop(&id);
    let results = orchestrator::start_with_deps(&state.procs, &state.registry, &id);
    Json(serde_json::json!({
        "ok": results.iter().all(|r| r.ok),
        "id": id,
        "details": results,
    }))
}

/// 按依赖拓扑顺序启动全部常驻服务。
pub async fn start_all(State(state): State<Arc<AppState>>) -> Json<Vec<orchestrator::ActionResult>> {
    Json(orchestrator::start_all(&state.procs, &state.registry))
}

/// 按逆依赖顺序停止全部服务。
pub async fn stop_all(State(state): State<Arc<AppState>>) -> Json<Vec<orchestrator::ActionResult>> {
    Json(orchestrator::stop_all(&state.procs, &state.registry))
}

pub async fn build_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    match &def.build_cmd {
        Some(cmd) => {
            let mut p = PathBuf::from(&def.install_dir);
            p.push("build.log");
            match state.procs.build(&def.install_dir, cmd, &p.to_string_lossy()) {
                Ok(_) => Ok(Json(serde_json::json!({"ok": true, "id": id}))),
                Err(e) => Ok(Json(serde_json::json!({"ok": false, "error": e.to_string()}))),
            }
        }
        None => Ok(Json(serde_json::json!({"ok": false, "error": "该服务未配置 buildCmd"}))),
    }
}

/// 读取指定服务的构建日志（前端轮询以展示构建进度）。
pub async fn build_log(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    let mut p = PathBuf::from(&def.install_dir);
    p.push("build.log");
    let text = if p.exists() {
        std::fs::read_to_string(&p).unwrap_or_default()
    } else {
        String::new()
    };
    Ok(Json(serde_json::json!({"id": id, "logs": text})))
}

pub async fn service_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    let text = logs::tail_many(&def.log_paths, 200);
    Ok(Json(serde_json::json!({"id": id, "logs": text})))
}

pub async fn diagnosis(
    State(state): State<Arc<AppState>>,
) -> Json<serde_json::Value> {
    let mut status: HashMap<String, (bool, bool)> = HashMap::new();
    for d in &state.registry {
        let running = state.procs.is_running(&d.id);
        let s = to_status(d, running);
        status.insert(d.id.clone(), (running, s.healthy));
    }
    // 环境检查：探测各服务的工具链 + 外部依赖，并纳入诊断。
    let env_reports: Vec<environment::ServiceEnvReport> =
        state.registry.iter().map(environment::check_service_env).collect();
    let findings = diagnosis::diagnose(&state.registry, &status, &env_reports);

    // LLM 增强层：按「模型 → 接入点 → 供应商」动态解析调用参数，未配置时退化为规则结果。
    let mut llm_summary: Option<String> = None;
    let mut llm_note: Option<String> = None;
    let target = state.ai_models.lock().unwrap().active();
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

/// 检查单个服务的运行环境（工具链 + 外部依赖）。
pub async fn env_check(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<environment::ServiceEnvReport>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(environment::check_service_env(def)))
}

/// 检查全部服务的运行环境。
pub async fn env_check_all(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<environment::ServiceEnvReport>> {
    Json(state.registry.iter().map(environment::check_service_env).collect())
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
    match s.save(&state.config_dir) {
        Ok(_) => {
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

