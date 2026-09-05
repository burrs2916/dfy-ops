use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::extract::Request;
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};

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

/// 前端页面的合法来源白名单（CORS）。
///
/// 各来源的出处：
///   · `http://localhost:18720` / `http://127.0.0.1:18720` —— 开发态 Vite（vite.config.ts strictPort）
///   · `http://tauri.localhost` —— 打包态 **Windows / Android**（本项目的交付平台就是 Windows）
///   · `https://tauri.localhost` —— Windows 侧若启用 `use_https_scheme`
///   · `tauri://localhost` —— 打包态 **macOS / Linux**（本机开发走这条）
///
/// 注意：Tauri v2 的生产 origin **按平台分裂**，不是统一值。Windows 是 http://tauri.localhost，
/// macOS/Linux 是 tauri://localhost。只配其中一个，会导致另一平台的前端请求被 CORS 全部拦掉。
const ALLOWED_ORIGINS: [&str; 5] = [
    "http://localhost:18720",
    "http://127.0.0.1:18720",
    "http://tauri.localhost",
    "https://tauri.localhost",
    "tauri://localhost",
];

/// 运维 Web 端必须携带的自定义头及其约定值。
///
/// 作用：浏览器对「非安全头」会强制发起 OPTIONS 预检，预检由下面的 CORS 白名单裁决。
/// 于是用户浏览的任意第三方页面都无法驱动本机运维 API——否则它可以直接
/// `POST /api/services/{id}/build` 触发任意命令执行（run_cmd 来自 DFY_OPS_SERVICES 指向的
/// TOML），或者 `POST /api/services/stop-all` 把生产服务全部停掉。
pub const CLIENT_HEADER: &str = "x-dfy-ops-client";
pub const CLIENT_HEADER_VALUE: &str = "dfy-ops-web";

/// 拒绝缺少客户端标识头的请求。
///
/// 放行 OPTIONS 预检：预检本身不带自定义头，由 CORS 层按来源白名单裁决，
/// 非法来源在预检阶段就被拦下，根本走不到这里。
async fn require_client_header(req: Request, next: Next) -> Response {
    if req.method() == Method::OPTIONS {
        return next.run(req).await;
    }
    let authorized = req
        .headers()
        .get(CLIENT_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|v| v == CLIENT_HEADER_VALUE)
        .unwrap_or(false);
    if !authorized {
        tracing::warn!(
            "拒绝缺少 {} 头的请求：{} {}",
            CLIENT_HEADER,
            req.method(),
            req.uri().path()
        );
        return (
            StatusCode::FORBIDDEN,
            "缺少或错误的 X-Dfy-Ops-Client 头：本接口仅供 DFY 运维平台前端调用",
        )
            .into_response();
    }
    next.run(req).await
}

/// 构造 Axum 路由。运维 REST API 监听本地 127.0.0.1:18721。
///
/// 必须挂 CORS 层：前端页面源（开发态 http://localhost:18720 / 打包态 tauri 源）
/// 与 API 源 http://127.0.0.1:18721 属跨源，浏览器会先发 OPTIONS 预检；
/// 无 CORS 层时预检落到路由返回 405，所有请求都会被浏览器拦截。
///
/// 但**不能**用 `CorsLayer::permissive()`：permissive 等于允许任意来源，
/// 绑定 127.0.0.1 只挡得住外部网络，挡不住同机浏览器——用户访问任意恶意页面
/// 就能跨域驱动本机运维接口（启停服务、触发构建）。因此改为白名单 + 自定义头双重校验。
pub fn router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(
            ALLOWED_ORIGINS
                .iter()
                .filter_map(|o| o.parse::<HeaderValue>().ok()),
        ))
        .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
        .allow_headers([CONTENT_TYPE, HeaderName::from_static(CLIENT_HEADER)]);

    Router::new()
        .route("/api/health", axum::routing::get(handlers::server_health))
        .route("/api/services", axum::routing::get(handlers::list_services))
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
        .route(
            "/api/ai/config",
            axum::routing::get(handlers_ai::get_ai_config),
        )
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
        // 层的执行顺序是「后挂的在外层」：CORS 最外层，先裁决预检与来源白名单，
        // 通过后再由 require_client_header 校验自定义头。
        .layer(axum::middleware::from_fn(require_client_header))
        .layer(cors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::appsettings::AppSettings;
    use crate::ops::process::ProcManager;

    fn test_state() -> Arc<AppState> {
        let dir = std::env::temp_dir().join(format!("dfy-ops-api-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        Arc::new(AppState {
            // 空注册表：这些测试只校验鉴权与 CORS，不触碰任何真实服务进程。
            registry: vec![],
            procs: ProcManager::new(),
            ai_models: Arc::new(Mutex::new(AiModelConfig::default())),
            app_settings: Arc::new(Mutex::new(AppSettings::load(&dir))),
            config_dir: dir,
        })
    }

    /// 在随机端口起一个真实 server，返回 base url。
    async fn spawn_server() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = test_state();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router(state)).await;
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn rejects_request_without_client_header() {
        let base = spawn_server().await;
        let res = reqwest::get(format!("{base}/api/health")).await.unwrap();
        assert_eq!(
            res.status(),
            reqwest::StatusCode::FORBIDDEN,
            "缺少客户端标识头的请求必须被拒绝"
        );
    }

    #[tokio::test]
    async fn accepts_request_with_client_header() {
        let base = spawn_server().await;
        let client = reqwest::Client::new();
        let res = client
            .get(format!("{base}/api/health"))
            .header(CLIENT_HEADER, CLIENT_HEADER_VALUE)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_wrong_client_header_value() {
        let base = spawn_server().await;
        let client = reqwest::Client::new();
        let res = client
            .get(format!("{base}/api/health"))
            .header(CLIENT_HEADER, "something-else")
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);
    }

    /// 预检（OPTIONS）必须放行：它不带自定义头，由 CORS 层按来源白名单裁决。
    #[tokio::test]
    async fn preflight_allowed_for_known_origins() {
        let base = spawn_server().await;
        let client = reqwest::Client::new();
        // 覆盖各平台的真实 origin：Windows 交付走 http://tauri.localhost，
        // macOS/Linux 开发走 tauri://localhost，Vite 开发态走 localhost:18720。
        for origin in ALLOWED_ORIGINS {
            let res = client
                .request(reqwest::Method::OPTIONS, format!("{base}/api/services"))
                .header("Origin", origin)
                .header("Access-Control-Request-Method", "GET")
                .header("Access-Control-Request-Headers", CLIENT_HEADER)
                .send()
                .await
                .unwrap();
            let acao = res
                .headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            assert_eq!(acao, origin, "来源 {origin} 应在预检中被放行");
        }
    }

    /// 核心安全断言：未知来源不得拿到 ACAO，否则任意网页都能驱动本机运维接口。
    #[tokio::test]
    async fn preflight_denied_for_unknown_origin() {
        let base = spawn_server().await;
        let client = reqwest::Client::new();
        for evil in ["https://evil.example.com", "http://localhost:9999", "*"] {
            let res = client
                .request(reqwest::Method::OPTIONS, format!("{base}/api/services"))
                .header("Origin", evil)
                .header("Access-Control-Request-Method", "POST")
                .header("Access-Control-Request-Headers", CLIENT_HEADER)
                .send()
                .await
                .unwrap();
            let acao = res
                .headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            assert!(
                acao.is_empty() || acao == "null",
                "非法来源 {evil} 不应获得 ACAO，实际收到：{acao:?}"
            );
        }
    }

    /// 允许的来源必须能真正带上自定义头完成一次 GET（走通「预检 + 自定义头」全链路）。
    #[tokio::test]
    async fn allowed_origin_can_call_api_with_custom_header() {
        let base = spawn_server().await;
        let client = reqwest::Client::new();
        let res = client
            .get(format!("{base}/api/services"))
            .header("Origin", "http://tauri.localhost")
            .header(CLIENT_HEADER, CLIENT_HEADER_VALUE)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK);
        assert_eq!(
            res.headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("http://tauri.localhost")
        );
    }
}
