//! Redis 连接凭据托管：config/redis_credential.json。
//!
//! Redis 运维是**远程连接模式**：平台不要求 Redis 本地安装，默认指向
//! 192.168.56.101:6379（开发环境虚拟机，密码 dfy123456）；生产环境在
//! 「Redis 管理」页的「连接配置」里修改。所有 Redis 连接（原生 redis 客户端）
//! 统一从这里取凭据，与 mysql_credential 同一套托管模式。
//!
//! 存储为明文 JSON：本机运维平台的信任边界就是本机磁盘（与 mysql_credential
//! 同级保护），加密只增加伪安全感。

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedisCredential {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_password")]
    pub password: String,
}

fn default_host() -> String {
    "192.168.56.101".into()
}

fn default_port() -> u16 {
    6379
}

fn default_password() -> String {
    "dfy123456".into()
}

impl Default for RedisCredential {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            password: default_password(),
        }
    }
}

fn path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join("redis_credential.json")
}

/// 读凭据；文件不存在或损坏时回退默认（192.168.56.101 / dfy123456 / 6379）。
pub fn load(config_dir: &Path) -> RedisCredential {
    match std::fs::read(path(config_dir)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => RedisCredential::default(),
    }
}

pub fn save(config_dir: &Path, cred: &RedisCredential) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| format!("无法创建配置目录：{e}"))?;
    let bytes = serde_json::to_vec_pretty(cred).map_err(|e| format!("凭据序列化失败：{e}"))?;
    std::fs::write(path(config_dir), bytes).map_err(|e| format!("写入凭据文件失败：{e}"))
}
