use std::sync::Arc;

use axum::extract::{Path, State};
use axum::Json;

use crate::api::server::AppState;
use crate::ops::ai;
use crate::ops::aimodels::{AiEndpoint, AiModel, AiModelConfig, AiProvider};

// ===== AI 模型配置（三层：供应商 → 接入点 → 模型，web-craft ModelConfigPage 同构） =====

/// 读取完整 AI 模型配置（providers/endpoints/models/activeModelId）。
pub async fn get_ai_config(State(state): State<Arc<AppState>>) -> Json<AiModelConfig> {
    Json(state.ai_models.lock().unwrap().clone())
}

/// 通用响应：保存成功后返回最新配置（前端无需再拉一次）。
///
/// 配置写盘是同步文件 IO，走 blocking 池执行，避免占用 async worker 线程
/// （与 handlers::blocking 同一策略）。
async fn saved_config(state: &AppState, cfg: AiModelConfig) -> Json<serde_json::Value> {
    let config_dir = state.config_dir.clone();
    let ai_models = state.ai_models.clone();
    let result = tokio::task::spawn_blocking(move || match cfg.save(&config_dir) {
        Ok(_) => Ok(cfg),
        Err(e) => Err(e),
    })
    .await
    .expect("blocking task 执行失败（内部 panic）");
    match result {
        Ok(cfg) => {
            *ai_models.lock().unwrap() = cfg.clone();
            Json(serde_json::json!({ "ok": true, "config": cfg }))
        }
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

/// 新增/更新供应商（带 id 为更新）。
pub async fn save_provider(
    State(state): State<Arc<AppState>>,
    Json(mut p): Json<AiProvider>,
) -> Json<serde_json::Value> {
    if p.id.is_empty() {
        p.id = crate::ops::aimodels::now_id("pv");
    }
    p.name = p.name.trim().to_string();
    if p.name.is_empty() {
        return Json(serde_json::json!({ "ok": false, "error": "供应商名称不能为空" }));
    }
    let mut cfg = state.ai_models.lock().unwrap().clone();
    cfg.upsert_provider(p);
    saved_config(&state, cfg).await
}

/// 删除供应商（级联删除其接入点与模型）。
pub async fn delete_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let mut cfg = state.ai_models.lock().unwrap().clone();
    cfg.delete_provider(&id);
    saved_config(&state, cfg).await
}

/// 新增/更新接入点（带 id 为更新）。
pub async fn save_endpoint(
    State(state): State<Arc<AppState>>,
    Json(mut e): Json<AiEndpoint>,
) -> Json<serde_json::Value> {
    if e.id.is_empty() {
        e.id = crate::ops::aimodels::now_id("ep");
    }
    e.name = e.name.trim().to_string();
    e.base_url = e.base_url.trim().to_string();
    if e.name.is_empty() {
        return Json(serde_json::json!({ "ok": false, "error": "接入点名称不能为空" }));
    }
    if !state
        .ai_models
        .lock()
        .unwrap()
        .providers
        .iter()
        .any(|p| p.id == e.provider_id)
    {
        return Json(serde_json::json!({ "ok": false, "error": "供应商不存在" }));
    }
    let mut cfg = state.ai_models.lock().unwrap().clone();
    cfg.upsert_endpoint(e);
    saved_config(&state, cfg).await
}

/// 删除接入点（级联删除其下模型）。
pub async fn delete_endpoint(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let mut cfg = state.ai_models.lock().unwrap().clone();
    cfg.delete_endpoint(&id);
    saved_config(&state, cfg).await
}

/// 新增/更新模型（带 id 为更新）。
pub async fn save_model(
    State(state): State<Arc<AppState>>,
    Json(mut m): Json<AiModel>,
) -> Json<serde_json::Value> {
    if m.id.is_empty() {
        m.id = crate::ops::aimodels::now_id("model");
    }
    m.name = m.name.trim().to_string();
    m.ref_key = m.ref_key.trim().to_string();
    if m.name.is_empty() || m.ref_key.is_empty() {
        return Json(serde_json::json!({ "ok": false, "error": "模型名称与标识(refKey)不能为空" }));
    }
    let mut cfg = state.ai_models.lock().unwrap().clone();
    cfg.upsert_model(m);
    saved_config(&state, cfg).await
}

/// 删除模型。
pub async fn delete_model(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let mut cfg = state.ai_models.lock().unwrap().clone();
    cfg.delete_model(&id);
    saved_config(&state, cfg).await
}

/// 设为诊断用模型。
pub async fn set_active_model(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let model_id = body["modelId"].as_str().unwrap_or("").to_string();
    let mut cfg = state.ai_models.lock().unwrap().clone();
    if !cfg.models.iter().any(|m| m.id == model_id) {
        return Json(serde_json::json!({ "ok": false, "error": "模型不存在" }));
    }
    cfg.active_model_id = Some(model_id);
    saved_config(&state, cfg).await
}

/// 接入点连通性测试（GET /models）。
pub async fn test_endpoint(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let cfg = state.ai_models.lock().unwrap().clone();
    let Some(endpoint) = cfg.endpoints.iter().find(|e| e.id == id) else {
        return Json(serde_json::json!({ "ok": false, "error": "接入点不存在" }));
    };
    let Some(provider) = cfg.providers.iter().find(|p| p.id == endpoint.provider_id) else {
        return Json(serde_json::json!({ "ok": false, "error": "所属供应商不存在" }));
    };
    match ai::test_endpoint(endpoint, &provider.api_key).await {
        Ok(msg) => Json(serde_json::json!({ "ok": true, "message": msg })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

/// 模型级测试（真实发一条 chat 消息；动态解析 model→endpoint→provider）。
pub async fn test_model(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let cfg = state.ai_models.lock().unwrap().clone();
    let Some(target) = cfg.resolve(&id) else {
        return Json(serde_json::json!({ "ok": false, "error": "模型不存在或归属链断裂" }));
    };
    let start = std::time::Instant::now();
    match ai::test_model(&target).await {
        Ok(msg) => Json(serde_json::json!({
            "ok": true,
            "message": format!("{} · {}（{} ms）", target.model_name, msg, start.elapsed().as_millis()),
        })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}
