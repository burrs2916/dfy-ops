//! 第三方服务远程端点托管：config/remote_endpoints.json。
//!
//! VerneMQ / MinIO 等第三方中间件部署在虚拟机或局域网主机上（与 Redis 的
//! redis_credential 同一套远程模式）：平台按这里托管的 host:port 做服务
//! 状态探测（TCP 可达 = 运行中），不再依赖本机进程/端口。
//!
//! 与 redis_credential 的关系：Redis 的端点与密码继续由 redis_credential
//! 托管，本文件管 vernemq / minio 等服务，含可选访问密码（空 = 无认证）。
//!
//! 存储为明文 JSON：本机运维平台的信任边界就是本机磁盘（与 *_credential
//! 同级保护），加密只增加伪安全感。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteEndpoint {
    pub host: String,
    pub port: u16,
    /// 可选访问密码（MinIO 等）：空 = 无认证。明文存储，信任边界与本机
    /// 配置目录一致（与 *_credential 同级），加密只增加伪安全感。
    #[serde(default)]
    pub password: String,
}

fn path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join("remote_endpoints.json")
}

fn load_all(config_dir: &Path) -> BTreeMap<String, RemoteEndpoint> {
    match std::fs::read(path(config_dir)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => BTreeMap::new(),
    }
}

/// 读指定服务的远程端点；未配置或文件不存在返回 None（检测回退本机探测）。
pub fn load(config_dir: &Path, service_id: &str) -> Option<RemoteEndpoint> {
    load_all(config_dir).get(service_id).cloned()
}

/// 保存指定服务的远程端点（服务发现「采用」或手动配置入口写入）。
pub fn save(config_dir: &Path, service_id: &str, endpoint: &RemoteEndpoint) -> Result<(), String> {
    let mut all = load_all(config_dir);
    all.insert(service_id.to_string(), endpoint.clone());
    std::fs::create_dir_all(config_dir).map_err(|e| format!("无法创建配置目录：{e}"))?;
    let bytes =
        serde_json::to_vec_pretty(&all).map_err(|e| format!("端点序列化失败：{e}"))?;
    std::fs::write(path(config_dir), bytes).map_err(|e| format!("写入端点文件失败：{e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("dfy-ops-remote-ep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ep = RemoteEndpoint {
            host: "192.168.56.101".into(),
            port: 18830,
            password: String::new(),
        };
        save(&dir, "vernemq", &ep).unwrap();
        assert_eq!(load(&dir, "vernemq").unwrap().port, 18830);
        assert!(load(&dir, "minio").is_none());
        save(
            &dir,
            "minio",
            &RemoteEndpoint {
                host: "10.0.0.5".into(),
                port: 9000,
                password: "minio-secret".into(),
            },
        )
        .unwrap();
        assert_eq!(load(&dir, "minio").unwrap().password, "minio-secret");
        assert_eq!(load(&dir, "vernemq").unwrap().port, 18830);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_none() {
        let dir = std::env::temp_dir().join("dfy-ops-remote-ep-missing");
        assert!(load(&dir, "vernemq").is_none());
    }
}
