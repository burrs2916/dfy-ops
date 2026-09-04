use serde::{Deserialize, Serialize};
use std::path::Path;

/// 应用自身设置（「设置」页），与 AI 助手配置（llm.json）严格分离。
/// 持久化于 config_dir/settings.json，保存后热更新内存，立即生效。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppSettings {
    /// 服务总览（Dashboard）状态轮询间隔（秒）。
    pub status_refresh_secs: u64,
    /// 运维中心环境复探间隔（秒）。
    pub env_refresh_secs: u64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            status_refresh_secs: 5,
            env_refresh_secs: 15,
        }
    }
}

impl AppSettings {
    /// 加载：config_dir/settings.json，缺失或损坏时回退默认值。
    pub fn load(config_dir: &Path) -> Self {
        config_dir
            .join("settings.json")
            .exists()
            .then(|| std::fs::read_to_string(config_dir.join("settings.json")).ok())
            .flatten()
            .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
            .unwrap_or_default()
    }

    /// 持久化到 config_dir/settings.json。
    pub fn save(&self, config_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(config_dir).ok();
        let path = config_dir.join("settings.json");
        let s = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(path, s)
    }

    /// 约束到合法区间（防手改配置文件把轮询设成 0 或天文数字）。
    pub fn sanitize(&mut self) {
        self.status_refresh_secs = self.status_refresh_secs.clamp(2, 120);
        self.env_refresh_secs = self.env_refresh_secs.clamp(5, 600);
    }
}
