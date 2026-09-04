use serde::{Deserialize, Serialize};
use std::path::Path;

/// AI 模型配置 —— 三层结构（与 web-craft ModelConfigPage 完全同构）：
/// Provider(供应商，持 API Key) → Endpoint(接入点，持 API 类型 + BaseURL) → Model(模型，挂接入点下)。
/// 持久化于 config_dir/ai_models.json；保存即热更新内存。
///
/// 铁律：模型→配置的绑定必须动态解析（model → endpoint → provider），禁止写死、禁止缓存——
/// 同名模型靠 endpoint 归属区分，换供应商只需改一层。
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct AiModelConfig {
    pub providers: Vec<AiProvider>,
    pub endpoints: Vec<AiEndpoint>,
    pub models: Vec<AiModel>,
    /// 当前用于「一键诊断」的模型 id；None 时取第一个启用的模型。
    pub active_model_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiProvider {
    pub id: String,
    pub name: String,
    pub api_key: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiEndpoint {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    /// openai-completions | anthropic-messages | ollama
    pub api_type: String,
    pub base_url: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModel {
    pub id: String,
    pub endpoint_id: String,
    /// 展示名
    pub name: String,
    /// 调用时传给 API 的模型标识（如 gpt-4o-mini / deepseek-chat）
    pub ref_key: String,
    pub enabled: bool,
}

/// 动态解析出的完整调用参数（实时查，不缓存）。
#[derive(Clone, Debug)]
pub struct ResolvedLlm {
    pub api_type: String,
    pub base_url: String,
    pub api_key: String,
    pub model_ref: String,
    pub model_name: String,
}

impl AiModelConfig {
    /// 加载：config_dir/ai_models.json，缺失或损坏时回退空配置。
    pub fn load(config_dir: &Path) -> Self {
        config_dir
            .join("ai_models.json")
            .exists()
            .then(|| std::fs::read_to_string(config_dir.join("ai_models.json")).ok())
            .flatten()
            .and_then(|s| serde_json::from_str::<AiModelConfig>(&s).ok())
            .unwrap_or_default()
    }

    /// 持久化到 config_dir/ai_models.json。
    pub fn save(&self, config_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir).ok();
        let path = config_dir.join("ai_models.json");
        let s = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, s)
    }

    /// upsert 供应商；返回写入后的配置引用不变。
    pub fn upsert_provider(&mut self, p: AiProvider) {
        match self.providers.iter_mut().find(|x| x.id == p.id) {
            Some(x) => *x = p,
            None => self.providers.push(p),
        }
    }

    pub fn upsert_endpoint(&mut self, e: AiEndpoint) {
        match self.endpoints.iter_mut().find(|x| x.id == e.id) {
            Some(x) => *x = e,
            None => self.endpoints.push(e),
        }
    }

    pub fn upsert_model(&mut self, m: AiModel) {
        match self.models.iter_mut().find(|x| x.id == m.id) {
            Some(x) => *x = m,
            None => self.models.push(m),
        }
    }

    /// 删除供应商（级联删除其接入点与模型；若删除的是 active 模型则清空指针）。
    pub fn delete_provider(&mut self, id: &str) {
        let ep_ids: Vec<String> = self
            .endpoints
            .iter()
            .filter(|e| e.provider_id == id)
            .map(|e| e.id.clone())
            .collect();
        self.models
            .retain(|m| !ep_ids.contains(&m.endpoint_id));
        self.endpoints.retain(|e| e.provider_id != id);
        self.providers.retain(|p| p.id != id);
        self.clear_active_if_gone();
    }

    pub fn delete_endpoint(&mut self, id: &str) {
        self.models.retain(|m| m.endpoint_id != id);
        self.endpoints.retain(|e| e.id != id);
        self.clear_active_if_gone();
    }

    pub fn delete_model(&mut self, id: &str) {
        self.models.retain(|m| m.id != id);
        self.clear_active_if_gone();
    }

    fn clear_active_if_gone(&mut self) {
        if let Some(aid) = &self.active_model_id {
            if !self.models.iter().any(|m| &m.id == aid) {
                self.active_model_id = None;
            }
        }
    }

    /// 动态解析：model → endpoint → provider，实时查（不缓存）。
    pub fn resolve(&self, model_id: &str) -> Option<ResolvedLlm> {
        let model = self.models.iter().find(|m| m.id == model_id)?;
        let endpoint = self.endpoints.iter().find(|e| e.id == model.endpoint_id)?;
        let provider = self.providers.iter().find(|p| p.id == endpoint.provider_id)?;
        Some(ResolvedLlm {
            api_type: endpoint.api_type.clone(),
            base_url: endpoint.base_url.clone(),
            api_key: provider.api_key.clone(),
            model_ref: model.ref_key.clone(),
            model_name: model.name.clone(),
        })
    }

    /// 取用于诊断的模型：active 指针优先，否则第一个启用的模型。
    pub fn active(&self) -> Option<ResolvedLlm> {
        let id = self
            .active_model_id
            .clone()
            .or_else(|| self.models.iter().find(|m| m.enabled).map(|m| m.id.clone()))?;
        self.resolve(&id)
    }
}

pub fn now_id(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{}", prefix, n)
}
