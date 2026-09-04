use crate::ops::aimodels::{AiEndpoint, ResolvedLlm};

/// 调用 LLM 对诊断结果做根因分析与处置建议。
/// 参数由 AiModelConfig 动态解析（model → endpoint → provider），未配置时返回 Err（调用方降级规则引擎）。
pub async fn analyze(target: &ResolvedLlm, prompt: String) -> Result<String, String> {
    match target.api_type.as_str() {
        "anthropic-messages" => analyze_anthropic(target, prompt).await,
        // openai-completions / ollama 均为 OpenAI 兼容协议
        _ => analyze_openai(target, prompt).await,
    }
}

async fn analyze_openai(target: &ResolvedLlm, prompt: String) -> Result<String, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/chat/completions", target.base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": target.model_ref,
        "messages": [{ "role": "user", "content": prompt }],
        "temperature": 0.2,
    });
    let resp = client
        .post(&url)
        .bearer_auth(&target.api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.map_err(|e| format!("响应解析失败: {e}"))?;
    if !status.is_success() {
        let msg = json["error"]["message"].as_str().unwrap_or("未知错误");
        return Err(format!("HTTP {status}: {msg}"));
    }
    let text = json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if text.is_empty() {
        return Err("模型返回了空内容".into());
    }
    Ok(text)
}

async fn analyze_anthropic(target: &ResolvedLlm, prompt: String) -> Result<String, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/v1/messages", target.base_url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": target.model_ref,
        "max_tokens": 2048,
        "messages": [{ "role": "user", "content": prompt }],
    });
    let resp = client
        .post(&url)
        .header("x-api-key", &target.api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("请求失败: {e}"))?;
    let status = resp.status();
    let json: serde_json::Value = resp.json().await.map_err(|e| format!("响应解析失败: {e}"))?;
    if !status.is_success() {
        let msg = json["error"]["message"].as_str().unwrap_or("未知错误");
        return Err(format!("HTTP {status}: {msg}"));
    }
    let text = json["content"][0]["text"].as_str().unwrap_or("").to_string();
    if text.is_empty() {
        return Err("模型返回了空内容".into());
    }
    Ok(text)
}

/// 接入点连通性测试：openai 系 GET {base}/models；anthropic GET {base}/v1/models。
pub async fn test_endpoint(endpoint: &AiEndpoint, api_key: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let (url, is_anthropic) = if endpoint.api_type == "anthropic-messages" {
        (
            format!("{}/v1/models", endpoint.base_url.trim_end_matches('/')),
            true,
        )
    } else {
        (
            format!("{}/models", endpoint.base_url.trim_end_matches('/')),
            false,
        )
    };
    let mut req = client.get(&url);
    req = if is_anthropic {
        req.header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
    } else {
        req.bearer_auth(api_key)
    };
    let resp = req
        .send()
        .await
        .map_err(|e| format!("无法连接: {e}"))?;
    let status = resp.status();
    if status.is_success() {
        Ok(format!("连接成功（HTTP {status}）"))
    } else if status.as_u16() == 401 || status.as_u16() == 403 {
        Err(format!("鉴权失败（HTTP {status}）：请检查 API Key"))
    } else {
        Err(format!("HTTP {status}"))
    }
}

/// 模型级测试：真实发一条 chat 消息。
pub async fn test_model(target: &ResolvedLlm) -> Result<String, String> {
    let reply = analyze(
        target,
        "这是一次连通性测试，请只回复两个字符：OK".to_string(),
    )
    .await?;
    Ok(format!("模型可用，回复：{}", reply.chars().take(40).collect::<String>()))
}
