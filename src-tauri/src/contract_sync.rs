//! 契约同步测试（P2-e）——把 Rust DTO 序列化后的【字段名与枚举值】钉死。
//!
//! 背景：后端 DTO 与前端 `src/types.ts` 是手工双写，没有 codegen。历史上已出现
//! 两次静默漂移的苗头（TS 侧 `RunState` 缺失、`Finding.severity` 多出 Rust 端
//! 不存在的 `info`）。本测试在每个双写 DTO 上断言：序列化 JSON 的键集合与
//! 枚举取值必须与 types.ts 完全一致——Rust 侧任何字段改名/增删都会让测试变红，
//! 强制同步 types.ts，杜绝契约漂移。
//!
//! 维护约定：在 Rust 里增删 DTO 字段时，改完这里跑红 → 同步改 `src/types.ts`
//! → 再改这里的期望列表使其变绿。三步缺一不可。
use serde_json::{json, Value};

use crate::api::handlers::{RunState, ServiceStatus};
use crate::ops::aimodels::{AiEndpoint, AiModel, AiModelConfig, AiProvider};
use crate::ops::appsettings::AppSettings;
use crate::ops::diagnosis::{Finding, Severity};
use crate::ops::environment::{CheckItem, CheckStatus, ServiceEnvReport};
use crate::ops::orchestrator::ActionResult;
use crate::ops::registry::ServiceKind;

fn sorted_keys(v: &Value) -> Vec<String> {
    let mut k: Vec<String> = v
        .as_object()
        .expect("DTO 应序列化为 JSON 对象")
        .keys()
        .cloned()
        .collect();
    k.sort();
    k
}

/// 断言 JSON 键集合与期望完全一致（双向：多字段、少字段都算漂移）。
fn assert_keys(v: &Value, expected: &[&str]) {
    let mut e: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
    e.sort();
    assert_eq!(
        sorted_keys(v),
        e,
        "契约漂移：Rust DTO 字段与 src/types.ts 不一致，两处必须同步修改"
    );
}

/// ServiceStatus ↔ types.ts `ServiceStatus`（含 runState 三态枚举）。
#[test]
fn service_status_contract() {
    let s = ServiceStatus {
        id: "x".into(),
        name: "x".into(),
        kind: ServiceKind::Vertx,
        running: true,
        run_state: RunState::Detected,
        healthy: true,
        ports: vec![1],
        depends_on: vec!["y".into()],
        is_daemon: true,
        third_party: false,
        description: String::new(),
        install_dir: String::new(),
        build_cmd: None,
        run_cmd: String::new(),
        health_url: None,
        log_paths: vec![],
    };
    assert_keys(
        &serde_json::to_value(&s).unwrap(),
        &[
            "id", "name", "kind", "running", "runState", "healthy", "ports", "dependsOn",
            "isDaemon", "thirdParty", "description", "installDir", "buildCmd", "runCmd",
            "healthUrl", "logPaths",
        ],
    );
    // RunState 枚举值
    assert_eq!(
        serde_json::to_value(RunState::Managed).unwrap(),
        json!("managed")
    );
    assert_eq!(
        serde_json::to_value(RunState::Detected).unwrap(),
        json!("detected")
    );
    assert_eq!(serde_json::to_value(RunState::Down).unwrap(), json!("down"));
}

/// ServiceEnvReport / CheckItem / CheckStatus ↔ types.ts 同名接口。
#[test]
fn env_report_contract() {
    let report = ServiceEnvReport {
        id: "x".into(),
        name: "x".into(),
        overall: CheckStatus::Pass,
        items: vec![CheckItem {
            id: "iid".into(),
            name: "n".into(),
            category: "c".into(),
            status: CheckStatus::Warn,
            expected: "e".into(),
            actual: "a".into(),
            is_fatal: false,
            fix_hint: "h".into(),
        }],
    };
    let v = serde_json::to_value(&report).unwrap();
    assert_keys(&v, &["id", "name", "overall", "items"]);
    assert_keys(
        &v["items"][0],
        &["id", "name", "category", "status", "expected", "actual", "isFatal", "fixHint"],
    );
    for (val, expect) in [
        (CheckStatus::Pass, "pass"),
        (CheckStatus::Warn, "warn"),
        (CheckStatus::Fail, "fail"),
    ] {
        assert_eq!(serde_json::to_value(val).unwrap(), json!(expect));
    }
}

/// Finding / Severity ↔ types.ts `Finding`（TS 侧仅 warning|critical，Rust 不产 Info）。
#[test]
fn finding_contract() {
    let f = Finding {
        service: "x".into(),
        severity: Severity::Warning,
        message: "m".into(),
        fix_hint: Some("h".into()),
    };
    assert_keys(
        &serde_json::to_value(&f).unwrap(),
        &["service", "severity", "message", "fixHint"],
    );
    assert_eq!(
        serde_json::to_value(Severity::Warning).unwrap(),
        json!("warning")
    );
    assert_eq!(
        serde_json::to_value(Severity::Critical).unwrap(),
        json!("critical")
    );
}

/// ActionResult ↔ types.ts `ActionResult`。
#[test]
fn action_result_contract() {
    let r = ActionResult {
        id: "x".into(),
        ok: true,
        error: None,
    };
    assert_keys(&serde_json::to_value(&r).unwrap(), &["id", "ok", "error"]);
}

/// AI 配置脱敏视图 ↔ types.ts AiModelConfigDto / AiProviderDto / AiEndpointDto / AiModelDto。
/// 关键点：public_config 的 keySet 只存在于该视图（AiProvider 本身没有此字段），
/// 测试钉的是 public_config 的输出形状。
#[test]
fn ai_config_public_contract() {
    let cfg = AiModelConfig {
        providers: vec![AiProvider {
            id: "p".into(),
            name: "p".into(),
            api_key: "sk-x".into(),
            enabled: true,
        }],
        endpoints: vec![AiEndpoint {
            id: "e".into(),
            provider_id: "p".into(),
            name: "e".into(),
            api_type: "openai-completions".into(),
            base_url: "https://api.example.com".into(),
            enabled: true,
        }],
        models: vec![AiModel {
            id: "m".into(),
            endpoint_id: "e".into(),
            name: "m".into(),
            ref_key: "gpt-x".into(),
            enabled: true,
        }],
        active_model_id: None,
    };
    let v = cfg.public_config();
    assert_keys(&v, &["providers", "endpoints", "models", "activeModelId"]);
    assert_keys(&v["providers"][0], &["id", "name", "apiKey", "keySet", "enabled"]);
    assert_keys(
        &v["endpoints"][0],
        &["id", "providerId", "name", "apiType", "baseUrl", "enabled"],
    );
    assert_keys(
        &v["models"][0],
        &["id", "endpointId", "name", "refKey", "enabled"],
    );
    // 脱敏铁律：keySet 为 true 时 apiKey 必须为空串
    assert_eq!(v["providers"][0]["apiKey"], json!(""));
    assert_eq!(v["providers"][0]["keySet"], json!(true));
}

/// AppSettings ↔ types.ts `AppSettings`。
#[test]
fn app_settings_contract() {
    let s = AppSettings::default();
    assert_keys(
        &serde_json::to_value(&s).unwrap(),
        &["statusRefreshSecs", "envRefreshSecs"],
    );
}
