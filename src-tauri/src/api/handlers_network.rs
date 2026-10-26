//! 局域网/虚拟机服务发现：网卡枚举 → 网段扫描 → 协议识别。
//!
//! 第三方中间件（Redis / VerneMQ / MinIO）部署在虚拟机或局域网主机上，
//! 用户未必知道虚拟机 IP——本模块按「本机网卡所在网段」并发 TCP 探测
//! 常见服务端口，连通的端口再走协议级握手确认服务身份（防误报），
//! 前端把结果一键「采用」到对应服务的远程端点配置。
//!
//! 扫描引擎：std 阻塞 connect 挪到 spawn_blocking + Semaphore 限并发，
//! 不占用 tokio worker；单连接超时 500ms，/24 网段约 3~5 秒出全量结果。

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::api::server::AppState;

/// 单次 TCP 连接超时：500ms 在局域网/虚拟网内绰绰有余，
/// 又能把 /24×4 端口的总耗时压在数秒级。
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

/// 扫描并发上限：128 个在途连接。/24 网段 254 主机 × 4 端口 ≈ 1016 次
/// connect，一轮 8 批 × 500ms ≈ 4 秒；再高收益递减且容易触发本机防火墙。
const SCAN_CONCURRENCY: usize = 128;

/// 网段规模上限：前缀 ≥22（每网段 ≤1024 主机）。拒绝 /16 这类
/// 6 万 IP 的巨量网段——扫描一分钟起步，用户体验和内网礼貌性都不允许。
const MAX_HOSTS: u32 = 1024;
/// 与 MAX_HOSTS 对应的最小前缀：2^(32-22) = 1024。
const MIN_PREFIX: u8 = 22;

/// 发现目标：平台托管的第三方服务端口 → 识别器。
/// VerneMQ 兼容自定义 18830 与标准 1883；MinIO 是 9000；Redis 是 6379。
const DEFAULT_TARGETS: &[(u16, &str)] = &[
    (6379, "redis"),
    (18830, "vernemq"),
    (1883, "vernemq"),
    (9000, "minio"),
    (4000, "greptime"),
];

// ===== 网卡枚举 =====

#[derive(Serialize)]
pub struct AdapterInfo {
    name: String,
    ip: String,
    /// 网段 CIDR（如 192.168.56.0/24）——直接作为 discover 的扫描输入。
    cidr: String,
    /// 该网段内的可扫描主机数（排除网络地址与广播地址）。
    hosts: u32,
}

/// 本机所有 IPv4 网卡（含 VirtualBox Host-Only / VMware VMnet 等虚拟网卡）。
/// 回环与 169.254 链路本地没有扫描价值，直接排除。
pub async fn adapters() -> Json<serde_json::Value> {
    let list = tokio::task::spawn_blocking(|| {
        netdev::get_interfaces()
            .into_iter()
            .flat_map(|iface| {
                let name = iface.name;
                iface.ipv4.into_iter().filter_map(move |net| {
                    let ip = net.addr();
                    if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() {
                        return None;
                    }
                    let prefix = net.prefix_len();
                    Some(AdapterInfo {
                        hosts: host_count(prefix),
                        name: name.clone(),
                        ip: ip.to_string(),
                        cidr: format!("{}/{prefix}", net.network()),
                    })
                })
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    Json(serde_json::json!({ "adapters": list }))
}

/// 网段内可扫描主机数（去掉网络地址与广播地址）；前缀非法时按 0 处理。
fn host_count(prefix: u8) -> u32 {
    if prefix >= 31 {
        0
    } else {
        (1u32 << (32 - prefix as u32)) - 2
    }
}

// ===== 网段扫描 =====

#[derive(Deserialize)]
pub struct DiscoverReq {
    /// 扫描网段 CIDR（来自 adapters 接口或手填），如 "192.168.56.0/24"。
    pub cidr: String,
}

#[derive(Serialize)]
struct DiscoveredService {
    port: u16,
    service: String,
    detail: String,
}

#[derive(Serialize)]
struct DiscoveredHost {
    ip: String,
    services: Vec<DiscoveredService>,
}

/// 网段扫描：并发探测每个 IP 的目标端口，连通的做协议识别。
///
/// 引导语义（用户明确要求）：网段是扫描的前提，缺失时返回带明确补救
/// 指引的错误，而不是笼统的参数报错——前端据此提示用户补齐这一步。
pub async fn discover(Json(req): Json<DiscoverReq>) -> Json<serde_json::Value> {
    let cidr = req.cidr.trim().to_string();
    if cidr.is_empty() {
        return Json(serde_json::json!({
            "ok": false,
            "error": "尚未指定扫描网段：请先在上方选择本机网卡对应的网段，平台才能扫描发现其中的 Redis（6379）、VerneMQ（18830）、MinIO（9000）、GreptimeDB（4000）服务。",
            "missing": "cidr"
        }));
    }
    let (network, prefix) = match parse_cidr(&cidr) {
        Ok(v) => v,
        Err(e) => return Json(serde_json::json!({ "ok": false, "error": e })),
    };
    let hosts = enumerate_hosts(network, prefix);
    let scanned = hosts.len();
    let started = Instant::now();
    let found = scan_hosts(hosts).await;
    Json(serde_json::json!({
        "ok": true,
        "cidr": format!("{network}/{prefix}"),
        "scanned": scanned,
        "elapsedMs": started.elapsed().as_millis() as u64,
        "hosts": found,
    }))
}

/// "a.b.c.d/p" → (网络地址, 前缀)。校验信息面向用户，逐条说清怎么改。
fn parse_cidr(s: &str) -> Result<(Ipv4Addr, u8), String> {
    let (ip_part, prefix_part) = s
        .split_once('/')
        .ok_or_else(|| format!("网段「{s}」缺少前缀长度：请按 a.b.c.d/前缀 填写（如 192.168.56.0/24）"))?;
    let prefix: u8 = prefix_part
        .trim()
        .parse()
        .map_err(|_| format!("前缀长度「{prefix_part}」不是 0~32 的数字"))?;
    if prefix > 32 {
        return Err(format!("前缀长度 {prefix} 超出 0~32 范围"));
    }
    let ip: Ipv4Addr = ip_part
        .trim()
        .parse()
        .map_err(|_| format!("网段地址「{ip_part}」不是合法 IPv4 地址"))?;
    if prefix < MIN_PREFIX {
        return Err(format!(
            "网段过大（/{prefix} 约 {} 台主机）：为避免长时间全网扫描，请选择 /22 及更小的网段（虚拟机通常在 /24 内）",
            host_count(prefix)
        ));
    }
    Ok((ip, prefix))
}

/// 枚举网段内主机地址（去掉网络地址与广播地址），上限 MAX_HOSTS。
fn enumerate_hosts(network: Ipv4Addr, prefix: u8) -> Vec<Ipv4Addr> {
    let base = u32::from(network) & mask_of(prefix);
    let size = 1u32 << (32 - prefix as u32);
    let size = size.min(MAX_HOSTS.max(2));
    (1..size - 1).map(|i| Ipv4Addr::from(base + i)).collect()
}

fn mask_of(prefix: u8) -> u32 {
    if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix as u32)
    }
}

/// 并发扫描：Semaphore 限流 + spawn_blocking 阻塞 connect。
/// 每个主机收敛为一行（端口升序），连通端口再走协议识别确认服务身份。
async fn scan_hosts(hosts: Vec<Ipv4Addr>) -> Vec<DiscoveredHost> {
    let semaphore = Arc::new(Semaphore::new(SCAN_CONCURRENCY));
    let mut handles = Vec::with_capacity(hosts.len() * DEFAULT_TARGETS.len());
    for ip in hosts {
        for (port, service) in DEFAULT_TARGETS {
            let sem = semaphore.clone();
            handles.push(tokio::task::spawn(async move {
                let _permit = sem.acquire_owned().await.ok()?;
                let (port, service) = (*port, *service);
                if !port_open(ip, port).await {
                    return None;
                }
                let (service, detail) = identify(ip, port, service).await;
                Some((ip, DiscoveredService { port, service, detail }))
            }));
        }
    }
    let mut by_ip: BTreeMap<Ipv4Addr, Vec<DiscoveredService>> = BTreeMap::new();
    for handle in handles {
        if let Ok(Some((ip, svc))) = handle.await {
            by_ip.entry(ip).or_default().push(svc);
        }
    }
    by_ip
        .into_iter()
        .map(|(ip, mut services)| {
            services.sort_by_key(|s| s.port);
            DiscoveredHost {
                ip: ip.to_string(),
                services,
            }
        })
        .collect()
}

async fn port_open(ip: Ipv4Addr, port: u16) -> bool {
    let addr = SocketAddr::new(IpAddr::V4(ip), port);
    tokio::task::spawn_blocking(move || TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).is_ok())
        .await
        .unwrap_or(false)
}

async fn identify(ip: Ipv4Addr, port: u16, fallback: &str) -> (String, String) {
    let addr = SocketAddr::new(IpAddr::V4(ip), port);
    let kind = fallback.to_string();
    let result = tokio::task::spawn_blocking(move || match kind.as_str() {
        "redis" => probe_redis(addr),
        "minio" => probe_minio(addr),
        "vernemq" => probe_mqtt(addr),
        _ => Ok(("open".into(), "端口开放".into())),
    })
    .await
    .unwrap_or_else(|_| Err("探测任务失败".into()));
    match result {
        Ok((service, detail)) => (service, detail),
        Err(e) => (fallback.to_string(), e),
    }
}

/// RESP PING：`+PONG` 即 Redis；未认证实例返回 `-NOAUTH ...` 同样证明身份。
fn probe_redis(addr: SocketAddr) -> Result<(String, String), String> {
    let mut s = connect(addr)?;
    use std::io::{Read, Write};
    s.write_all(b"*1\r\n$4\r\nPING\r\n")
        .map_err(|e| format!("发送 PING 失败：{e}"))?;
    let mut buf = [0u8; 128];
    let n = s.read(&mut buf).map_err(|e| format!("读取响应失败：{e}"))?;
    let resp = String::from_utf8_lossy(&buf[..n]);
    if resp.starts_with("+PONG") {
        Ok(("Redis".into(), "PING 已确认".into()))
    } else if resp.contains("NOAUTH") || resp.contains("ERR") {
        Ok(("Redis".into(), "需要访问密码".into()))
    } else {
        Err("响应不是 Redis 协议".into())
    }
}

/// MinIO 探测：HTTP GET /，响应中带 MinIO 特征（Server 头或 XML 命名空间）。
fn probe_minio(addr: SocketAddr) -> Result<(String, String), String> {
    let mut s = connect(addr)?;
    use std::io::{Read, Write};
    let req = format!("GET / HTTP/1.0\r\nHost: {}\r\nUser-Agent: dfy-ops-discover\r\nConnection: close\r\n\r\n", addr);
    s.write_all(req.as_bytes())
        .map_err(|e| format!("发送 HTTP 请求失败：{e}"))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 512];
    while buf.len() < 2048 {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let resp = String::from_utf8_lossy(&buf);
    if resp.to_ascii_lowercase().contains("minio") {
        Ok(("MinIO".into(), "HTTP 特征已确认".into()))
    } else {
        Err("响应无 MinIO 特征（可能是其他 HTTP 服务占用 9000）".into())
    }
}

/// MQTT 3.1.1 CONNECT 握手：收到 CONNACK（0x20 首字节）即确认 MQTT broker。
/// 固定编码 CONNECT 包：协议名 "MQTT" + level 4 + clean session + keepalive 60，
/// client id "dfy-ops"（服务端只回 CONNACK，不会留存订阅等副作用）。
fn probe_mqtt(addr: SocketAddr) -> Result<(String, String), String> {
    let mut s = connect(addr)?;
    use std::io::{Read, Write};
    let connect_packet: &[u8] = &[
        0x10, 0x12, // CONNECT，剩余长度 18
        0x00, 0x04, b'M', b'Q', b'T', b'T', // 协议名
        0x04, // level 4（MQTT 3.1.1）
        0x02, // clean session
        0x00, 0x3C, // keepalive 60s
        0x00, 0x06, b'd', b'f', b'y', b'-', b'o', b'p', b's', // client id "dfy-ops"
    ];
    s.write_all(connect_packet)
        .map_err(|e| format!("发送 MQTT CONNECT 失败：{e}"))?;
    let mut buf = [0u8; 4];
    let n = s.read(&mut buf).map_err(|e| format!("读取响应失败：{e}"))?;
    if n >= 1 && buf[0] & 0xF0 == 0x20 {
        Ok(("VerneMQ".into(), "MQTT 握手已确认".into()))
    } else {
        Err("响应不是 MQTT CONNACK".into())
    }
}

fn connect(addr: SocketAddr) -> Result<TcpStream, String> {
    let s = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        .map_err(|e| format!("连接失败：{e}"))?;
    s.set_read_timeout(Some(CONNECT_TIMEOUT)).ok();
    s.set_write_timeout(Some(CONNECT_TIMEOUT)).ok();
    Ok(s)
}

// ===== 第三方服务远程端点 =====

// ===== 扫描网段配置 =====

/// 上次使用的扫描网段持久化（config/network_scan.json）。
///
/// 生产环境网段变化时（换路由器/虚拟机网络模式切换），用户在 UI 手动输入
/// 新网段扫描即可，无需重新编译——这里记住上次输入，下次打开直接带入。
fn scan_config_path(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join("network_scan.json")
}

fn load_last_cidr(config_dir: &Path) -> String {
    match std::fs::read(scan_config_path(config_dir)) {
        Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| v.get("lastCidr").and_then(|c| c.as_str().map(String::from)))
            .unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// 查询上次使用的扫描网段（空串表示从未扫描过）。
pub async fn get_scan_config(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "lastCidr": load_last_cidr(&state.config_dir) }))
}

#[derive(Deserialize)]
pub struct SaveScanConfigReq {
    pub last_cidr: String,
}

/// 保存上次使用的扫描网段（扫描成功时前端自动回写）。
pub async fn save_scan_config(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SaveScanConfigReq>,
) -> Json<serde_json::Value> {
    let cidr = req.last_cidr.trim().to_string();
    if !cidr.is_empty() && parse_cidr(&cidr).is_err() {
        return Json(serde_json::json!({
            "ok": false,
            "error": "网段格式无法识别：请按 a.b.c.d/前缀 填写（如 192.168.56.0/24）"
        }));
    }
    std::fs::create_dir_all(&state.config_dir).ok();
    let bytes = serde_json::to_vec_pretty(&serde_json::json!({ "lastCidr": cidr }))
        .map_err(|e| e.to_string());
    match bytes.and_then(|b| {
        std::fs::write(scan_config_path(&state.config_dir), b).map_err(|e| e.to_string())
    }) {
        Ok(()) => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": format!("写入网段配置失败：{e}") })),
    }
}

// ===== 第三方服务远程端点（网关地址配置） =====

/// 服务的远程端点读取口径（与 handlers.rs 的检测同一来源）：
/// Redis 端点在 redis_credential（密码同文件），其余第三方在 remote_endpoints。
/// Redis 从未保存过连接配置时返回 None（与检测的「先设置后检测」口径一致），
/// 否则配置面板会把默认兜底地址当成已配置展示，误导用户。
pub fn service_remote(state: &AppState, service_id: &str) -> Option<(String, u16)> {
    if service_id == "redis" {
        if !crate::ops::redis_credential::is_configured(&state.config_dir) {
            return None;
        }
        let cred = crate::ops::redis_credential::load(&state.config_dir);
        return Some((cred.host, cred.port));
    }
    crate::ops::remote_endpoints::load(&state.config_dir, service_id)
        .map(|e| (e.host, e.port))
}

/// 查询服务当前远程端点（未配置返回 null，前端据此显示「未配置」引导）。
/// 密码不回显明文，只返回是否已设置。
pub async fn get_remote(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(service_id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    if service_id == "redis" {
        if !crate::ops::redis_credential::is_configured(&state.config_dir) {
            return Json(serde_json::json!({ "host": null, "port": null, "password_set": false }));
        }
        let cred = crate::ops::redis_credential::load(&state.config_dir);
        return Json(serde_json::json!({
            "host": cred.host,
            "port": cred.port,
            "password_set": !cred.password.is_empty(),
        }));
    }
    match crate::ops::remote_endpoints::load(&state.config_dir, &service_id) {
        Some(e) => Json(serde_json::json!({
            "host": e.host,
            "port": e.port,
            "password_set": !e.password.is_empty(),
        })),
        None => Json(serde_json::json!({ "host": null, "port": null, "password_set": false })),
    }
}

#[derive(Deserialize)]
pub struct SaveRemoteReq {
    pub host: String,
    pub port: u16,
    /// 可选访问密码：空 = 无认证 / 保持 Redis 已有密码不变
    #[serde(default)]
    pub password: String,
}

/// 保存服务远程端点：服务发现「采用」与手动配置共用入口。
/// Redis 写 redis_credential（密码仅在填写时更新），其余写 remote_endpoints
/// （密码为可选字段，空 = 无认证）。
pub async fn save_remote(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(service_id): axum::extract::Path<String>,
    Json(req): Json<SaveRemoteReq>,
) -> Json<serde_json::Value> {
    let host = req.host.trim().to_string();
    if host.is_empty() {
        return Json(serde_json::json!({ "ok": false, "error": "主机地址不能为空" }));
    }
    let result = if service_id == "redis" {
        let mut cred = crate::ops::redis_credential::load(&state.config_dir);
        cred.host = host;
        cred.port = req.port;
        if !req.password.trim().is_empty() {
            cred.password = req.password.trim().to_string();
        }
        crate::ops::redis_credential::save(&state.config_dir, &cred)
    } else {
        crate::ops::remote_endpoints::save(
            &state.config_dir,
            &service_id,
            &crate::ops::remote_endpoints::RemoteEndpoint {
                host,
                port: req.port,
                password: req.password.trim().to_string(),
            },
        )
    };
    match result {
        Ok(()) => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_parse_and_hosts() {
        assert_eq!(parse_cidr("192.168.56.0/24").unwrap().0.to_string(), "192.168.56.0");
        assert_eq!(parse_cidr("192.168.56.0/24").unwrap().1, 24);
        assert!(parse_cidr("192.168.56.0").is_err());
        assert!(parse_cidr("192.168.56.0/abc").is_err());
        assert!(parse_cidr("10.0.0.0/8").is_err());
        assert_eq!(host_count(24), 254);
        assert_eq!(host_count(22), 1022);
    }

    #[test]
    fn enumerate_excludes_network_and_broadcast() {
        let (net, prefix) = parse_cidr("192.168.56.0/24").unwrap();
        let hosts = enumerate_hosts(net, prefix);
        assert_eq!(hosts.len(), 254);
        assert_eq!(hosts[0].to_string(), "192.168.56.1");
        assert_eq!(hosts[253].to_string(), "192.168.56.254");
    }

    #[test]
    fn oversize_cidr_error_is_actionable() {
        // 错误信息必须给出补救指引（前缀范围），而不是只说"格式错误"
        let err = parse_cidr("10.0.0.0/8").unwrap_err();
        assert!(err.contains("/22"), "错误提示应说明可选前缀范围：{err}");
    }
}
