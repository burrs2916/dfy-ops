use serde::{Deserialize, Serialize};
use std::path::Path;

/// 应用自身设置（「设置」页），与 AI 助手配置（llm.json）严格分离。
/// 持久化于 config_dir/settings.json，保存后热更新内存，立即生效。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppSettings {
    /// 服务总览状态自动轮询间隔（秒）；0 = 关闭（默认）——启动时检测一次，
    /// 此后按需手动/单服务刷新。全量探测在 Windows 上开销大，常驻轮询得不偿失。
    pub status_refresh_secs: u64,
    /// 运维中心环境复探间隔（秒）；0 = 关闭（默认），仅进入页面时检测。
    pub env_refresh_secs: u64,
    /// 本地资源目录：存放环境修复所需的离线安装包（MySQL/Redis/VerneMQ/MinIO/
    /// JDK 等的 MSI/EXE/ZIP）。交付机常无外网，修复时优先扫描此目录离线安装；
    /// 空串 = 未配置（跳过本地来源，直接在线/AI 兜底）。
    pub resource_dir: String,
    /// 声明为「远程部署」的软件（如 ["redis","minio"]）：本机不安装、修复中心
    /// 不再显示其安装引导与"未安装"噪音，仅保留说明与改回本地的入口。
    pub remote_deployed: Vec<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            status_refresh_secs: 0,
            env_refresh_secs: 0,
            resource_dir: String::new(),
            remote_deployed: Vec::new(),
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

    /// 约束到合法区间（防手改配置文件把轮询设成天文数字）。
    /// 0 合法（= 关闭自动轮询）；>0 时钳到合理下限，避免 1 秒级高频探测。
    pub fn sanitize(&mut self) {
        if self.status_refresh_secs > 0 {
            self.status_refresh_secs = self.status_refresh_secs.clamp(2, 120);
        }
        if self.env_refresh_secs > 0 {
            self.env_refresh_secs = self.env_refresh_secs.clamp(5, 600);
        }
        self.resource_dir = self.resource_dir.trim().trim_matches('"').to_string();
        // 远程部署清单：小写化 + 只保留已知目标，防手改配置引入脏值
        self.remote_deployed = self
            .remote_deployed
            .iter()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| crate::ops::resource::TARGETS.contains(&s.as_str()))
            .collect();
    }
}
