use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use axum::extract::Request;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::api::handlers;
use crate::api::handlers_ai;
use crate::api::handlers_mysql;
use crate::api::handlers_network;
use crate::api::handlers_redis;
use crate::api::handlers_repair;
use crate::ops::aimodels::AiModelConfig;
use crate::ops::env_repair::{EnvCache, RepairState};
use crate::ops::process::ProcManager;
use crate::ops::registry::ServiceDef;

/// 共享应用状态：服务注册表 + 进程管理器 + AI 模型配置（三层，可热更新）+ 应用设置（可热更新）+ 配置目录。
pub struct AppState {
    pub registry: Vec<ServiceDef>,
    /// Arc：保活看门狗线程与 API 共享同一进程管理器。
    pub procs: Arc<ProcManager>,
    pub ai_models: Arc<Mutex<AiModelConfig>>,
    pub app_settings: Arc<Mutex<crate::ops::appsettings::AppSettings>>,
    pub config_dir: PathBuf,
    /// 环境检查全量结果缓存：全量探测含子进程调用，60s TTL 内直接复用。
    pub env_cache: EnvCache,
    /// env_check_all 的 single-flight 锁：缓存过期瞬间的并发请求只放一个重探，
    /// 其余等锁后命中新缓存（tokio 异步锁，等待不占 worker 线程）。
    pub env_rebuild: Arc<tokio::sync::Mutex<()>>,
    /// 跨请求共享的环境探测缓存（java -version / nginx -v 等子进程调用，60s TTL）：
    /// 全量与单服务检查复用同一事实（单服务检查不再每次冷启 java）；
    /// force 全量重检时清空——「重新检查」必须拿到装 JDK 后的新事实。
    pub probes: crate::ops::environment::ProbeCache,
    /// 环境修复任务状态（JDK 自动安装等；全局单例，前端轮询）。
    pub repair: Arc<RepairState>,
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

/// 请求日志的耗时阈值（ms）：超过即记 slow——检测卡顿、探测超时的第一手线索。
const SLOW_LOG_MS: u128 = 1000;
/// 响应体读取上限：JSON 业务错误体（须查 {"ok":false}）/ 文本错误体。
/// 超限的响应只记状态与耗时，不读正文（大日志文件内容不能被截断破坏）。
const JSON_BODY_LIMIT: usize = 512 * 1024;
const TEXT_BODY_LIMIT: usize = 64 * 1024;

fn clamp_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

/// 从 JSON 响应体提取业务错误消息——仅当 `{"ok": false}`（本项目 handler 的失败惯例）。
/// 这类失败以 HTTP 200 返回，状态码全绿，不读正文就永远不会留痕。
/// 返回 error/message 字段（缺失则整段正文），超长截断。
fn json_fail_message(bytes: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    if v.get("ok").and_then(|o| o.as_bool())? {
        return None; // ok=true（或无 ok 字段）→ 业务成功，不是失败
    }
    let msg = v
        .get("error")
        .and_then(|e| e.as_str())
        .or_else(|| v.get("message").and_then(|m| m.as_str()))
        .map(str::to_string)
        .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned());
    Some(clamp_str(&msg, 400))
}

/// 请求日志中间件（挂最外层：CORS 拦截、缺客户端头 403 的请求也能记到）。
///
/// 补的盲区：本项目大量 handler 以 HTTP 200 + `{"ok":false,"error":...}` 返回业务失败——
/// 状态码全绿，此前任何地方都不留痕，出错时 debug.log 一片安静，根本无从定位。
/// 落 `[HTTP]` scope 四类（`phase=api`）：
///   · http_fail —— 4xx/5xx（缺客户端头、CORS 拦截、参数 400…），带响应错误文本
///   · resp_fail —— 200 但 `{"ok":false}`，带 error 字段
///   · slow      —— 耗时 ≥1s（检测卡顿、探测超时的线索）
///   · action    —— 成功的变更请求（POST/DELETE）留痕，用于还原「出错前做过什么」
/// 成功的 GET 轮询不记（5s 一轮，纯噪音）。
/// **请求体一律不落日志**——可能含密码、SQL、凭据。
async fn log_api_requests(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let uri = req.uri().to_string();
    let started = Instant::now();
    let mut res = next.run(req).await;
    let ms = started.elapsed().as_millis();
    let status = res.status();

    let is_json = res
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.starts_with("application/json"))
        .unwrap_or(false);
    let failed = status.is_client_error() || status.is_server_error();
    let slow = ms >= SLOW_LOG_MS;
    let mutating = method != Method::GET && method != Method::HEAD && method != Method::OPTIONS;

    // 需要读响应体的场景：JSON（查 ok=false）/ 失败或慢的文本响应（错误文本入日志）
    let limit = if is_json {
        JSON_BODY_LIMIT
    } else if failed || slow {
        TEXT_BODY_LIMIT
    } else {
        0
    };

    let mut fail_msg: Option<String> = None;
    let too_big = res
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .map(|n| n > limit)
        .unwrap_or(false);
    if limit > 0 && !too_big {
        let (parts, body) = res.into_parts();
        match axum::body::to_bytes(body, limit).await {
            Ok(bytes) => {
                if is_json {
                    fail_msg = json_fail_message(&bytes);
                } else if failed {
                    fail_msg = Some(clamp_str(&String::from_utf8_lossy(&bytes), 400));
                }
                // 原字节原样重建响应，读日志/查数据的调用方无感
                res = Response::from_parts(parts, axum::body::Body::from(bytes));
            }
            Err(_) => {
                // 极端情况（流中断/超限）：状态与错误仍要留下，正文放弃
                res = Response::from_parts(parts, axum::body::Body::empty());
                fail_msg = Some("响应体读取失败（超限或流中断）".into());
            }
        }
    }

    let (level, evt) = if status.is_server_error() {
        ("ERROR", "http_fail")
    } else if status.is_client_error() {
        ("WARN", "http_fail")
    } else if fail_msg.is_some() {
        ("WARN", "resp_fail")
    } else if slow {
        ("WARN", "slow")
    } else if mutating {
        ("INFO", "action")
    } else {
        return res; // 成功的只读轮询：不落日志
    };

    let mut detail = format!("{} {} -> {} {}ms", method, uri, status.as_u16(), ms);
    if let Some(m) = fail_msg {
        detail.push_str(&format!(" | {m}"));
    }
    crate::debug_log::entry(crate::debug_log::HTTP_SCOPE, level, "api", evt, &detail);
    res
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
        .route("/api/diagnostics", axum::routing::get(handlers::diagnostics))
        .route(
            "/api/diagnostics/open-log-dir",
            axum::routing::post(handlers::diagnostics_open_log_dir),
        )
        .route("/api/services", axum::routing::get(handlers::list_services))
        .route("/api/ops-events", axum::routing::get(handlers_repair::ops_events_list))
        .route("/api/ops-events", axum::routing::delete(handlers_repair::ops_events_clear))
        .route(
            "/api/ops-events/:id",
            axum::routing::delete(handlers_repair::ops_events_delete),
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
        // 监控中心：日志文件清单（服务 × 文件元信息）
        .route("/api/log-files", axum::routing::get(handlers::log_files))
        // 监控中心日志维护：备份/删除（白名单校验同读取）
        .route(
            "/api/services/:id/logs/backup",
            axum::routing::post(handlers::log_backup),
        )
        .route(
            "/api/services/:id/logs/delete",
            axum::routing::post(handlers::log_delete),
        )
        // 清空日志内容（文件被占用删不掉时的兜底，文件本体保留）
        .route(
            "/api/services/:id/logs/clear",
            axum::routing::post(handlers::log_clear),
        )
        // 归档管理：批量清理归档/衍生日志（可保留最近 N 天，主日志永不删）
        .route(
            "/api/services/:id/logs/cleanup-archives",
            axum::routing::post(handlers::log_cleanup_archives),
        )
        .route("/api/diagnosis", axum::routing::get(handlers::diagnosis))
        .route("/api/env", axum::routing::get(handlers::env_check_all))
        .route(
            "/api/services/:id/env",
            axum::routing::get(handlers::env_check),
        )
        // 单服务组合刷新：一次快照同时出状态与环境（启停后局部刷新走这条，减半等待）
        .route(
            "/api/services/:id/refresh",
            axum::routing::get(handlers::service_refresh),
        )
        // 云端服务外部化配置（config/application-local.yaml）：读 / 原子写回
        .route(
            "/api/services/:id/external-config",
            axum::routing::get(handlers::external_config_get).post(handlers::external_config_update),
        )
        // 打开服务配置所在目录（文件管理器定位，只读便利操作）
        .route(
            "/api/services/:id/open-config-dir",
            axum::routing::post(handlers::open_config_dir),
        )
        // 一键打开平台页面（按 nginx 实际监听端口生成 URL，默认浏览器打开）
        .route("/api/platform/open", axum::routing::post(handlers::platform_open))
        // 一键诊断快照（版本+服务状态+端口占用+最近事件，复制给维护方即可定位）
        .route(
            "/api/diagnostics/snapshot",
            axum::routing::get(handlers::diagnostics_snapshot),
        )
        // 环境修复：POST 启动（分级：无需修复/配置指引/自动安装），GET 轮询任务状态
        .route(
            "/api/env/repair",
            axum::routing::post(handlers::start_env_repair),
        )
        .route(
            "/api/env/repair/status",
            axum::routing::get(handlers::env_repair_status),
        )
        // 环境修复中心：资源目录扫描 / 通用修复任务 / AI 结构化方案 / 白名单代执行
        .route(
            "/api/env/resources",
            axum::routing::get(handlers_repair::resources),
        )
        .route(
            "/api/env/repair/generic",
            axum::routing::post(handlers_repair::start_generic_repair),
        )
        .route(
            "/api/env/repair/plan",
            axum::routing::post(handlers_repair::repair_plan),
        )
        .route(
            "/api/env/repair/exec",
            axum::routing::post(handlers_repair::repair_exec),
        )
        // 卸载 / 维护：安装痕迹探测 + 卸载任务（停服务/注销服务/删目录）
        .route(
            "/api/env/repair/uninstall/status",
            axum::routing::get(handlers_repair::uninstall_status),
        )
        .route(
            "/api/env/repair/uninstall",
            axum::routing::post(handlers_repair::uninstall),
        )
        // 数据库运维：服务启停（net start/stop + UAC）与 SQL 导入（stdin 直灌）
        .route(
            "/api/env/repair/service",
            axum::routing::post(handlers_repair::service_action),
        )
        .route(
            "/api/env/repair/mysql/import",
            axum::routing::post(handlers_repair::mysql_import),
        )
        .route(
            "/api/env/repair/mysql/import/analyze",
            axum::routing::post(handlers_repair::mysql_import_analyze),
        )
        .route(
            "/api/env/repair/mysql/import/conflicts",
            axum::routing::post(handlers_repair::mysql_import_conflicts),
        )
        .route(
            "/api/env/repair/mysql/import/verify",
            axum::routing::post(handlers_repair::mysql_import_verify),
        )
        .route(
            "/api/env/repair/mysql/import/progress",
            axum::routing::get(handlers_repair::mysql_import_progress),
        )
        // JDK 环境变量一键修复（JAVA_HOME + PATH，幂等）
        .route(
            "/api/env/repair/java-env",
            axum::routing::post(handlers_repair::java_env_fix),
        )
        // 数据库运维中心（MySQL）：SQL 控制台 / 库表浏览 / 用户权限 / 密码托管
        .route("/api/mysql/overview", axum::routing::get(handlers_mysql::overview))
        .route("/api/mysql/exec", axum::routing::post(handlers_mysql::exec))
        .route("/api/mysql/tables", axum::routing::get(handlers_mysql::tables))
        .route("/api/mysql/users", axum::routing::get(handlers_mysql::users))
        .route(
            "/api/mysql/create-user",
            axum::routing::post(handlers_mysql::create_user),
        )
        .route(
            "/api/mysql/password",
            axum::routing::post(handlers_mysql::change_password),
        )
        .route(
            "/api/mysql/credential",
            axum::routing::get(handlers_mysql::credential),
        )
        // 库/表维护：建删库（删前自动备份）、表结构/DDL、数据分页、表操作、会话管理
        .route("/api/mysql/create-db", axum::routing::post(handlers_mysql::create_db))
        .route("/api/mysql/drop-db", axum::routing::post(handlers_mysql::drop_db))
        .route("/api/mysql/columns", axum::routing::get(handlers_mysql::columns))
        .route("/api/mysql/data", axum::routing::get(handlers_mysql::data))
        .route("/api/mysql/ddl", axum::routing::get(handlers_mysql::ddl))
        .route(
            "/api/mysql/table-action",
            axum::routing::post(handlers_mysql::table_action),
        )
        .route(
            "/api/mysql/processlist",
            axum::routing::get(handlers_mysql::processlist),
        )
        .route("/api/mysql/kill", axum::routing::post(handlers_mysql::kill))
        .route("/api/mysql/drop-user", axum::routing::post(handlers_mysql::drop_user))
        // 动态配置：托管连接修正 / 业务用户改密 / 常用参数在线调整（白名单）
        .route(
            "/api/mysql/user-password",
            axum::routing::post(handlers_mysql::user_password),
        )
        .route(
            "/api/mysql/credential",
            axum::routing::post(handlers_mysql::update_credential),
        )
        .route("/api/mysql/variables", axum::routing::get(handlers_mysql::variables))
        .route(
            "/api/mysql/set-variable",
            axum::routing::post(handlers_mysql::set_variable),
        )
        .route(
            "/api/mysql/test-connection",
            axum::routing::post(handlers_mysql::test_connection),
        )
        .route(
            "/api/mysql/user-grants",
            axum::routing::post(handlers_mysql::user_grants),
        )
        .route(
            "/api/mysql/config-file",
            axum::routing::get(handlers_mysql::get_config_file),
        )
        .route(
            "/api/mysql/config-file",
            axum::routing::post(handlers_mysql::save_config_file),
        )
        .route(
            "/api/mysql/backup",
            axum::routing::post(handlers_mysql::backup_now),
        )
        .route(
            "/api/mysql/backups",
            axum::routing::get(handlers_mysql::backups_list),
        )
        .route(
            "/api/mysql/restore",
            axum::routing::post(handlers_mysql::restore),
        )
        .route(
            "/api/mysql/export-csv",
            axum::routing::post(handlers_mysql::export_csv),
        )
        // Redis 远程运维（类 Another Redis Desktop Manager）：凭据托管 /
        // 测试连接 / 总览 / SCAN 遍历 / Key 详情 / 删除 / TTL
        .route("/api/redis/credential", axum::routing::get(handlers_redis::credential))
        .route(
            "/api/redis/credential",
            axum::routing::post(handlers_redis::save_credential),
        )
        .route("/api/redis/test", axum::routing::post(handlers_redis::test))
        .route("/api/redis/overview", axum::routing::get(handlers_redis::overview))
        .route("/api/redis/keys", axum::routing::get(handlers_redis::keys))
        .route(
            "/api/redis/key",
            axum::routing::get(handlers_redis::key_detail),
        )
        .route(
            "/api/redis/delete",
            axum::routing::post(handlers_redis::delete_key),
        )
        .route("/api/redis/ttl", axum::routing::post(handlers_redis::set_ttl))
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
        // 服务发现域：网卡枚举 / 网段扫描（Redis·VerneMQ·MinIO 协议级识别）/
        // 第三方服务远程端点（虚拟机部署场景的服务状态检测目标）
        .route(
            "/api/network/adapters",
            axum::routing::get(handlers_network::adapters),
        )
        .route(
            "/api/network/discover",
            axum::routing::post(handlers_network::discover),
        )
        .route(
            "/api/network/scan-config",
            axum::routing::get(handlers_network::get_scan_config),
        )
        .route(
            "/api/network/scan-config",
            axum::routing::post(handlers_network::save_scan_config),
        )
        .route(
            "/api/services/:id/remote",
            axum::routing::get(handlers_network::get_remote),
        )
        .route(
            "/api/services/:id/remote",
            axum::routing::post(handlers_network::save_remote),
        )
        .route(
            "/api/services/:id/remote-test",
            axum::routing::post(handlers_network::remote_test),
        )
        .with_state(state)
        // 层的执行顺序是「后挂的在外层」：
        // require_client_header 校验自定义头 → CORS 裁决预检与来源白名单 →
        // 请求日志挂最外层（CORS 拒绝、缺客户端头 403 的响应都要路过它，都得留痕）。
        .layer(axum::middleware::from_fn(require_client_header))
        .layer(cors)
        .layer(axum::middleware::from_fn(log_api_requests))
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
            procs: Arc::new(ProcManager::new()),
            ai_models: Arc::new(Mutex::new(AiModelConfig::default())),
            app_settings: Arc::new(Mutex::new(AppSettings::load(&dir))),
            config_dir: dir,
            env_cache: Arc::new(Mutex::new(None)),
            env_rebuild: Arc::new(tokio::sync::Mutex::new(())),
            probes: crate::ops::environment::ProbeCache::default(),
            repair: Arc::new(crate::ops::env_repair::RepairState::default()),
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

    /// env_check_all 的 force 查询参数必须被正确解析：手动「重新检查」依赖它
    /// 跳过 60s 缓存——参数解析失败会退化成 400，前端复检拿不到任何数据。
    #[tokio::test]
    async fn env_check_accepts_force_query() {
        let base = spawn_server().await;
        let client = reqwest::Client::new();
        let res = client
            .get(format!("{base}/api/env?force=true"))
            .header(CLIENT_HEADER, CLIENT_HEADER_VALUE)
            .send()
            .await
            .unwrap();
        assert_eq!(
            res.status(),
            reqwest::StatusCode::OK,
            "?force=true 必须被接受为合法查询参数"
        );
        assert!(res.json::<serde_json::Value>().await.unwrap().is_array());
    }

    /// 业务失败以 200 + {"ok":false} 返回——日志中间件必须能从正文把它识别出来，
    /// 否则这类失败在 debug.log 里永远无痕（本项目最关键的日志盲区）。
    #[test]
    fn json_fail_detects_ok_false_with_error() {
        let msg = json_fail_message(r#"{"ok":false,"error":"SQL 执行失败：连接被拒绝"}"#.as_bytes())
            .expect("ok=false 必须被识别为业务失败");
        assert!(msg.contains("连接被拒绝"), "应提取 error 字段，实际：{msg}");
    }

    /// ok=true / 无 ok 字段的成功响应不得被误判为失败（否则日志全是假警报）。
    #[test]
    fn json_fail_ignores_success_payloads() {
        assert!(json_fail_message(r#"{"ok":true,"message":"完成"}"#.as_bytes()).is_none());
        assert!(json_fail_message(br#"{"status":"ok","services":3}"#).is_none());
        assert!(json_fail_message(b"[1,2,3]").is_none());
        assert!(json_fail_message(b"not json at all").is_none());
    }

    /// error 字段缺失时回退整段正文——错误现场宁可多留也不能空。
    #[test]
    fn json_fail_falls_back_to_body_when_no_error_field() {
        let msg = json_fail_message(r#"{"ok":false,"detail":"boom"}"#.as_bytes())
            .expect("ok=false 无 error 字段也要留正文");
        assert!(msg.contains("boom"), "应回退整段正文，实际：{msg}");
    }

    /// 超长错误消息必须截断——单行日志无限膨胀会把 debug.log 冲爆。
    #[test]
    fn json_fail_clamps_long_messages() {
        let long = "x".repeat(2000);
        let body = format!(r#"{{"ok":false,"error":"{long}"}}"#);
        let msg = json_fail_message(body.as_bytes()).unwrap();
        assert!(
            msg.chars().count() <= 401,
            "错误消息应截断到 400 字符左右，实际 {}",
            msg.chars().count()
        );
    }

    /// 端到端：失败请求必须真的落盘 debug.log（`[HTTP] phase=api evt=http_fail` 行）。
    /// 单元测试只证明分类函数正确，证明不了「中间件 → entry → 文件」整条链是通的。
    #[tokio::test]
    async fn failed_request_lands_in_debug_log() {
        use crate::debug_log;
        let dir = std::env::temp_dir().join(format!("dfy-ops-logtest-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("debug.log");
        let _ = std::fs::remove_file(&file); // 清掉上轮残留，防旧内容让断言假通过
        debug_log::init_paths(std::sync::Arc::new(vec![file.clone()]));

        let base = spawn_server().await;
        // 缺客户端头 → 403：确定性失败，不触碰任何真实服务
        let res = reqwest::get(format!("{base}/api/health")).await.unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::FORBIDDEN);

        let content = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(
            content
                .lines()
                .any(|l| l.contains("phase=api")
                    && l.contains("evt=http_fail")
                    && l.contains("/api/health")
                    && l.contains("403")),
            "403 失败请求必须在 debug.log 留下 [HTTP] 行，实际内容：\n{content}"
        );

        // tracing 桥同一路径验证：分段写入（fmt 的真实行为：先内容后换行）必须
        // 攒行落盘，且 ANSI 颜色码剥干净——否则发布态 debug.log 全是转义垃圾。
        {
            use tracing_subscriber::fmt::MakeWriter;
            let mut w = debug_log::TraceMakeWriter.make_writer();
            std::io::Write::write_all(&mut w, b"\x1b[32mINFO\x1b[0m bridge").unwrap();
            std::io::Write::write_all(&mut w, b" ok\n").unwrap();
            // 尾部残行（无换行）靠 Drop flush 落盘——fmt 偶发不补尾换行时不能丢内容
            std::io::Write::write_all(&mut w, b"tail-no-newline").unwrap();
            drop(w);
        }
        let content = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(
            content.contains("bridge ok"),
            "tracing 桥必须把行落到 debug.log，实际内容：\n{content}"
        );
        assert!(
            content.contains("tail-no-newline"),
            "无尾换行的残行必须靠 Drop flush 落盘"
        );
        assert!(
            !content.contains('\u{1b}'),
            "debug.log 不得残留 ANSI 转义码"
        );
    }
}
