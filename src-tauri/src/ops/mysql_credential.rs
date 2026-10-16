//! MySQL root 凭据托管：config/mysql_credential.json。
//!
//! 平台用 `--initialize-insecure` 初始化 MySQL（root 空密码），装好后由
//! 「数据库运维」页引导设置 root 密码，密码由此模块统一托管——所有 mysql
//! 客户端调用（SQL 控制台 / 导入 / 授权）都从这里取凭据，不再要求空密码。
//!
//! 存储为明文 JSON：本机运维平台的信任边界就是本机磁盘（与 my.ini、datadir
//! 同级保护），加密只增加伪安全感。

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MysqlCredential {
    #[serde(default = "default_user")]
    pub user: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_user() -> String {
    "root".into()
}

fn default_port() -> u16 {
    3306
}

impl Default for MysqlCredential {
    fn default() -> Self {
        Self {
            user: default_user(),
            password: String::new(),
            port: default_port(),
        }
    }
}

fn path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join("mysql_credential.json")
}

/// 读凭据；文件不存在或损坏时回退默认（root / 空密码 / 3306，即初始安装态）。
pub fn load(config_dir: &Path) -> MysqlCredential {
    match std::fs::read(path(config_dir)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => MysqlCredential::default(),
    }
}

pub fn save(config_dir: &Path, cred: &MysqlCredential) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| format!("无法创建配置目录：{e}"))?;
    let bytes = serde_json::to_vec_pretty(cred).map_err(|e| format!("凭据序列化失败：{e}"))?;
    std::fs::write(path(config_dir), bytes).map_err(|e| format!("写入凭据文件失败：{e}"))
}
