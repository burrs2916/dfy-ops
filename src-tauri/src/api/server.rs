use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::Router;
use tower_http::cors::CorsLayer;

use crate::api::handlers;
use crate::api::handlers_ai;
use crate::ops::aimodels::AiModelConfig;
use crate::ops::process::ProcManager;
use crate::ops::registry::ServiceDef;

/// 共享应用状态：服务注册表 + 进程管理器 + AI 模型配置（三层，可热更新）+ 应用设置（可热更新）+ 配置目录。
pub struct AppState {
    pub registry: Vec<ServiceDef>,
    pub procs: ProcManager,
    pub ai_models: Arc<Mutex<AiModelConfig>>,
    pub app_settings: Arc<Mutex<crate::ops::appsettings::AppSettings>>,
    pub config_dir: PathBuf,
}

/// 构造 Axum 路由。运维 REST API 监听本地 127.0.0.1:18721。
///
/// 必须挂 CORS 层：前端页面源（开发态 http://localhost:18720 / 打包态 tauri://localhost）
/// 与 API 源 http://127.0.0.1:18721 属跨源，浏览器会先发 OPTIONS 预检；
/// 无 CORS 层时预检落到路由返回 405，所有请求都会被浏览器拦截。
/// 服务仅绑定 127.0.0.1，外部不可达，故预检策略用 permissive 是安全的。
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", axum::routing::get(handlers::server_health))
        .route(
            "/api/services",
            axum::routing::get(handlers::list_services),
        )
        .route(
            "/api/services/:id",
            axum::routing::get(handlers::get_service),
        )
        .route(
            "/api/services/:id/start",
            axum::routing::post(handlers::start_service),
        )
        .route(
            "/api/services/:id/stop",
            axum::routing::post(handlers::stop_service),
        )
        .route(
            "/api/services/:id/restart",
            axum::routing::post(handlers::restart_service),
        )
        .route(
            "/api/services/:id/build",
            axum::routing::post(handlers::build_service),
        )
        .route(
            "/api/services/:id/build-log",
            axum::routing::get(handlers::build_log),
        )
        .route(
            "/api/services/start-all",
            axum::routing::post(handlers::start_all),
        )
        .route(
            "/api/services/stop-all",
            axum::routing::post(handlers::stop_all),
        )
        .route(
            "/api/services/:id/logs",
            axum::routing::get(handlers::service_logs),
        )
        .route("/api/diagnosis", axum::routing::get(handlers::diagnosis))
        .route("/api/env", axum::routing::get(handlers::env_check_all))
        .route(
            "/api/services/:id/env",
            axum::routing::get(handlers::env_check),
        )
        .route(
            "/api/settings/app",
            axum::routing::get(handlers::get_app_settings),
        )
        .route(
            "/api/settings/app",
            axum::routing::post(handlers::save_app_settings),
        )
        // AI 助手域：三层模型配置（供应商→接入点→模型），与应用「设置」严格分离
        .route("/api/ai/config", axum::routing::get(handlers_ai::get_ai_config))
        .route(
            "/api/ai/providers",
            axum::routing::post(handlers_ai::save_provider),
        )
        .route(
            "/api/ai/providers/:id",
            axum::routing::delete(handlers_ai::delete_provider),
        )
        .route(
            "/api/ai/endpoints",
            axum::routing::post(handlers_ai::save_endpoint),
        )
        .route(
            "/api/ai/endpoints/:id",
            axum::routing::delete(handlers_ai::delete_endpoint),
        )
        .route(
            "/api/ai/endpoints/:id/test",
            axum::routing::post(handlers_ai::test_endpoint),
        )
        .route(
            "/api/ai/models",
            axum::routing::post(handlers_ai::save_model),
        )
        .route(
            "/api/ai/models/:id",
            axum::routing::delete(handlers_ai::delete_model),
        )
        .route(
            "/api/ai/models/:id/test",
            axum::routing::post(handlers_ai::test_model),
        )
        .route(
            "/api/ai/active-model",
            axum::routing::post(handlers_ai::set_active_model),
        )
        .with_state(state)
        .layer(CorsLayer::permissive())
}
