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
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs};
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
        "vernemq" => probe_mqtt(addr, "", "").map(|(ok, d)| if ok { ("VerneMQ".into(), d) } else { ("MQTT?".into(), d) }),
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

/// MQTT CONNECT 握手：用 rumqttc 同步 Client（真实 MQTT 客户端）完成连接并解析 CONNACK。
/// 返回 (是否握手成功, 反馈详情)：收到 CONNACK（无论接受与否）即证明端口上是
/// MQTT 服务；Err 表示连 MQTT 反馈都没有（连接被关闭 / 非 MQTT 响应 / 超时）。
///
/// connection_timeout = 7s：VerneMQ 的 auth_on_register webhook 默认 response_timeout = 5s，
/// Broker 等 webhook 后端回复完才发 CONNACK；5s 绰绰有余，留 2s 余量给网络。
fn probe_mqtt(addr: SocketAddr, username: &str, password: &str) -> Result<(bool, String), String> {
    use crate::debug_log as dl;
    use rumqttc::{Client, ConnectReturnCode, MqttOptions, Packet};

    let t_start = Instant::now();
    dl::entry(
        dl::MQTT_SCOPE, "INFO", "connect", "enter",
        &format!(
            "host={} port={} user={} pwd={}",
            addr.ip(), addr.port(),
            username, dl::secret_shape(Some(password))
        ),
    );

    let mut opts = MqttOptions::new("dfy-ops-probe", addr.ip().to_string(), addr.port());
    opts.set_keep_alive(Duration::from_secs(5));
    // 连接超时必须 > Broker 的 auth webhook response_timeout（VerneMQ 默认 5s）：
    // VerneMQ 收到 CONNECT 先等 auth_on_register webhook 回来才发 CONNACK，
    // rumqttc 默认 5s 连接超时恰好和 webhook 等待同时到点，webhook 慢一拍就
    // 客户端先报 Network timeout、收不到本该有的 CONNACK。留 3 倍余量。
    opts.set_connection_timeout(15);
    if !username.is_empty() || !password.is_empty() {
        opts.set_credentials(username, password);
    }

    dl::entry(dl::MQTT_SCOPE, "DEBUG", "connect", "client.new.start", "调用 rumqttc Client::new()");
    let (client, mut connection) = Client::new(opts, 10);
    dl::entry(
        dl::MQTT_SCOPE, "DEBUG", "connect", "client.new.ok",
        &format!("Client::new() 返回，已过 {}ms", t_start.elapsed().as_millis()),
    );

    // 等 CONNACK 最多 20s（> 连接超时 15s）：Broker 侧 auth webhook 最多等 5s，
    // 加上排队重试，20s 内没回基本就是 webhook 目标不通
    dl::entry(
        dl::MQTT_SCOPE, "INFO", "connect", "recv.start",
        "阻塞等 CONNACK，timeout=20s（连接超时 15s，容忍 Broker auth webhook 5s 等待）",
    );
    let first_event = match connection.recv_timeout(Duration::from_secs(20)) {
        Ok(Ok(ev)) => {
            dl::entry(
                dl::MQTT_SCOPE, "DEBUG", "connect", "recv.event",
                &format!("收到事件：{ev:?}"),
            );
            ev
        }
        Ok(Err(conn_err)) => {
            dl::entry(
                dl::MQTT_SCOPE, "ERROR", "connect", "recv.connerror",
                &format!("rumqttc ConnectionError：{conn_err}（elapsed {}ms）", t_start.elapsed().as_millis()),
            );
            let _ = client.try_disconnect();
            return Err(format!("Broker 反馈失败：{conn_err}"));
        }
        Err(timeout_err) => {
            dl::entry(
                dl::MQTT_SCOPE, "ERROR", "connect", "recv.timeout",
                &format!("recv_timeout 返回 RecvTimeoutError：{timeout_err:?}（elapsed {}ms）", t_start.elapsed().as_millis()),
            );
            let _ = client.try_disconnect();
            return Err(format!("Broker 反馈超时：{timeout_err:?}"));
        }
    };

    let _ = client.try_disconnect();
    let connack = match first_event {
        rumqttc::Event::Incoming(Packet::ConnAck(c)) => c,
        other => {
            dl::entry(
                dl::MQTT_SCOPE, "WARN", "connect", "recv.not.connack",
                &format!("首个事件不是 CONNACK：{other:?}（elapsed {}ms）", t_start.elapsed().as_millis()),
            );
            return Err(format!("未收到 CONNACK，而是：{other:?}"));
        }
    };

    let rc = connack.code;
    let rc_byte = rc as u8;
    let rc_name = match rc {
        ConnectReturnCode::Success => "Success",
        ConnectReturnCode::RefusedProtocolVersion => "RefusedProtocolVersion",
        ConnectReturnCode::BadClientId => "BadClientId",
        ConnectReturnCode::ServiceUnavailable => "ServiceUnavailable",
        ConnectReturnCode::BadUserNamePassword => "BadUserNamePassword",
        ConnectReturnCode::NotAuthorized => "NotAuthorized",
    };
    dl::entry(
        dl::MQTT_SCOPE, "INFO", "connect", "connack",
        &format!("rc=0x{rc_byte:02x} ({rc_name}) session_present={} elapsed={}ms",
            connack.session_present, t_start.elapsed().as_millis()),
    );

    match rc {
        ConnectReturnCode::Success => Ok((true, "MQTT 握手成功，Broker 已接受连接".into())),
        ConnectReturnCode::RefusedProtocolVersion => {
            Ok((false, "端口确认是 MQTT 服务，但协议版本未被接受（broker 不支持 MQTT 3.1.1）".into()))
        }
        ConnectReturnCode::BadClientId => {
            Ok((false, "端口确认是 MQTT 服务，但客户端标识被 Broker 拒绝".into()))
        }
        ConnectReturnCode::ServiceUnavailable => {
            Ok((false, "端口确认是 MQTT 服务，但 Broker 暂时不可用（可能达到连接上限）".into()))
        }
        ConnectReturnCode::BadUserNamePassword => {
            Ok((false, "端口确认是 MQTT 服务，但认证被拒：用户名或密码不正确".into()))
        }
        ConnectReturnCode::NotAuthorized => {
            Ok((false, "端口确认是 MQTT 服务，但该账号未获授权连接".into()))
        }
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
/// 用户名与密码回显明文：本机运维平台的信任边界就是本机磁盘（与 *_credential
/// 存储同级），密码框直接显示已存值（掩码 + 可切换明文），无需「留空不改」猜测语义。
pub async fn get_remote(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(service_id): axum::extract::Path<String>,
) -> Json<serde_json::Value> {
    if service_id == "redis" {
        if !crate::ops::redis_credential::is_configured(&state.config_dir) {
            return Json(serde_json::json!({ "host": null, "port": null, "username": "", "password": "" }));
        }
        let cred = crate::ops::redis_credential::load(&state.config_dir);
        return Json(serde_json::json!({
            "host": cred.host,
            "port": cred.port,
            "username": "",
            "password": cred.password,
        }));
    }
    match crate::ops::remote_endpoints::load(&state.config_dir, &service_id) {
        Some(e) => Json(serde_json::json!({
            "host": e.host,
            "port": e.port,
            "username": e.username,
            "password": e.password,
        })),
        None => Json(serde_json::json!({ "host": null, "port": null, "username": "", "password": "" })),
    }
}

#[derive(Deserialize)]
pub struct SaveRemoteReq {
    pub host: String,
    pub port: u16,
    /// 可选认证用户名（MQTT broker）：缺省（扫描采用）= 不改动已存值，
    /// 显式传值（含空串）= 覆盖。
    #[serde(default)]
    pub username: Option<String>,
    /// 可选访问密码：缺省（扫描采用）= 不改动已存密码，
    /// 显式传值（含空串）= 覆盖（用户在回显框里清空即主动删除密码）。
    #[serde(default)]
    pub password: Option<String>,
}

/// 保存服务远程端点：服务发现「采用」与手动配置共用入口。
/// Redis 写 redis_credential，其余写 remote_endpoints。username/password
/// 缺省时沿用已存值——服务发现只确认地址，绝不能悄悄抹掉已有凭据。
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
        if let Some(p) = req.password {
            cred.password = p.trim().to_string();
        }
        crate::ops::redis_credential::save(&state.config_dir, &cred)
    } else {
        let prev = crate::ops::remote_endpoints::load(&state.config_dir, &service_id);
        let username = match req.username {
            Some(u) => u.trim().to_string(),
            None => prev.as_ref().map(|e| e.username.clone()).unwrap_or_default(),
        };
        let password = match req.password {
            Some(p) => p.trim().to_string(),
            None => prev.as_ref().map(|e| e.password.clone()).unwrap_or_default(),
        };
        crate::ops::remote_endpoints::save(
            &state.config_dir,
            &service_id,
            &crate::ops::remote_endpoints::RemoteEndpoint { host, port: req.port, username, password },
        )
    };
    match result {
        Ok(()) => Json(serde_json::json!({ "ok": true })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

// ===== 远程端点连通性测试 =====

#[derive(Deserialize)]
pub struct RemoteTestReq {
    pub host: String,
    pub port: u16,
    /// 认证用户名（MQTT broker 等）：空则由测试逻辑按服务类型取默认值
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
}

/// 手动测试超时：测试按钮是用户显式等待的动作，比扫描的 500ms 宽裕
///（跨 VPN / 高负载虚拟机场景），2s 内不通基本就是真不通。
const TEST_TIMEOUT: Duration = Duration::from_secs(2);

/// 远程端点测试（用表单当前值，不落盘）：用户填完地址点「测试」即可知道配置对不对，
/// 不用先保存再盯着服务状态猜。各服务的验证深度与服务身份识别一致：
/// - Redis：AUTH（有密码时）+ PING——密码错误/需要密码能明确报出
/// - VerneMQ：真实客户端式 MQTT 握手（带用户名密码），CONNACK 返回码逐项解读——
///   认证被拒、guest 禁止远程登录等都有明确反馈，不再笼统「未确认」
/// - MinIO：HTTP 特征确认（访问密钥需 S3 签名才能验证，纯探测做不到，如实说明）
/// - GreptimeDB：HTTP GET /health 返回 2xx 确认
pub async fn remote_test(
    axum::extract::Path(service_id): axum::extract::Path<String>,
    Json(req): Json<RemoteTestReq>,
) -> Json<serde_json::Value> {
    let host = req.host.trim().to_string();
    if host.is_empty() {
        return Json(serde_json::json!({
            "ok": false,
            "error": "主机地址不能为空：请先填写虚拟机/远程主机的 IP 地址"
        }));
    }
    let res = tokio::task::spawn_blocking(move || {
        run_remote_test(&service_id, &host, req.port, req.username.trim(), req.password.trim())
    })
    .await
    .unwrap_or_else(|_| Err("测试任务执行失败".into()));
    match res {
        Ok((warn, detail)) => Json(serde_json::json!({ "ok": true, "warn": warn, "detail": detail })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

/// 同步执行一次端点测试：Ok((warn, detail)) —— warn=true 表示端口通但服务身份未确认。
fn run_remote_test(
    service_id: &str,
    host: &str,
    port: u16,
    username: &str,
    password: &str,
) -> Result<(bool, String), String> {
    use crate::debug_log as dl;
    let run_started = Instant::now();
    dl::entry(
        dl::MQTT_SCOPE, "INFO", "run", "enter",
        &format!("service={service_id} host={host}:{port} user={} pwd={}",
            username, dl::secret_shape(Some(password))),
    );

    let (warn, detail) = match service_id {
        "redis" => {
            let cred = crate::ops::redis_credential::RedisCredential {
                host: host.to_string(),
                port,
                password: password.to_string(),
            };
            let mut conn = crate::api::handlers_redis::open_conn(&cred)?;
            let pong: String = redis::cmd("PING")
                .query(&mut conn)
                .map_err(crate::api::handlers_redis::conn_err)?;
            if pong != "PONG" {
                return Err(format!("PING 未返回 PONG（收到：{pong}）"));
            }
            (false, "PING 已确认，认证通过".to_string())
        }
        "vernemq" => {
            dl::entry(dl::MQTT_SCOPE, "INFO", "run", "dns.resolve.start", &format!("host={host} port={port}"));
            let addr = match resolve_one(host, port) {
                Ok(a) => {
                    dl::entry(dl::MQTT_SCOPE, "INFO", "run", "dns.resolve.ok", &format!("resolved={a}"));
                    a
                }
                Err(e) => {
                    dl::entry(dl::MQTT_SCOPE, "ERROR", "run", "dns.resolve.fail", &e);
                    return Err(e);
                }
            };
            // 额外：先做一次 TCP 连通性探测，把网络层和应用层错误区分开
            dl::entry(dl::MQTT_SCOPE, "INFO", "run", "tcp.probe.start", &format!("addr={addr}"));
            match TcpStream::connect_timeout(&addr, Duration::from_secs(3)) {
                Ok(_stream) => {
                    dl::entry(dl::MQTT_SCOPE, "INFO", "run", "tcp.probe.ok", "TCP 三次握手成功");
                    // 立即 drop，让 rumqttc 自己建连接（我们不复用这个 stream）
                }
                Err(e) => {
                    let kind = e.kind();
                    dl::entry(
                        dl::MQTT_SCOPE, "ERROR", "run", "tcp.probe.fail",
                        &format!("TCP 连接失败 kind={kind:?} err={e}"),
                    );
                    // TCP 都连不上就不用试 MQTT 了
                    return Err(format!("TCP 连接 {addr} 失败（{kind:?}）：{e}"));
                }
            }

            let u = if username.is_empty() && !password.is_empty() {
                "guest"
            } else {
                username
            };
            dl::entry(dl::MQTT_SCOPE, "INFO", "run", "mqtt.probe.start",
                &format!("addr={addr} user={} pwd={}", u, dl::secret_shape(Some(password))));
            match probe_mqtt(addr, u, password) {
                Ok((true, d)) => {
                    dl::entry(dl::MQTT_SCOPE, "INFO", "run", "mqtt.probe.success", &d);
                    (false, d)
                }
                Ok((false, d)) => {
                    dl::entry(dl::MQTT_SCOPE, "WARN", "run", "mqtt.probe.rejected", &d);
                    (true, d)
                }
                Err(e) => {
                    dl::entry(dl::MQTT_SCOPE, "ERROR", "run", "mqtt.probe.error", &e);
                    (
                        true,
                        format!("端口连通，但未拿到 MQTT 握手反馈——{e}"),
                    )
                }
            }
        }
        "minio" => {
            let addr = resolve_one(host, port)?;
            match probe_minio(addr) {
                Ok((_, d)) => (false, format!("{d}（访问密钥需登录后验证）")),
                Err(_) => (
                    true,
                    "端口连通，但响应无 MinIO 特征——请核对端口是否为 MinIO API 端口".to_string(),
                ),
            }
        }
        "greptime" => match probe_http_status(host, port, "/health") {
            Ok(status) => (false, format!("HTTP /health 返回 {status}，服务已确认")),
            Err(_) => (
                true,
                "端口连通，但 /health 未返回正常状态——请核对端口是否为 GreptimeDB HTTP 端口"
                    .to_string(),
            ),
        },
        other => return Err(format!("服务「{other}」不支持远程端点测试")),
    };
    dl::entry(
        dl::MQTT_SCOPE, "INFO", "run", "done",
        &format!("warn={warn} detail={detail} elapsed={}ms", run_started.elapsed().as_millis()),
    );
    Ok((warn, format!("{detail}，耗时 {} ms", run_started.elapsed().as_millis())))
}

/// DNS 解析取第一个地址；解析失败给出面向用户的指引。
fn resolve_one(host: &str, port: u16) -> Result<SocketAddr, String> {
    (host, port)
        .to_socket_addrs()
        .map_err(|_| format!("主机名「{host}」无法解析——请检查地址拼写"))?
        .next()
        .ok_or_else(|| format!("主机名「{host}」没有可用的地址记录"))
}

/// TCP 连接 + 分类错误提示：拒绝 / 超时 / 其他，指向不同的排查方向。
fn tcp_probe(host: &str, port: u16) -> Result<TcpStream, String> {
    let addr = resolve_one(host, port)?;
    match TcpStream::connect_timeout(&addr, TEST_TIMEOUT) {
        Ok(s) => {
            s.set_read_timeout(Some(TEST_TIMEOUT)).ok();
            s.set_write_timeout(Some(TEST_TIMEOUT)).ok();
            Ok(s)
        }
        Err(e) => {
            let lower = e.to_string().to_ascii_lowercase();
            if lower.contains("refused") {
                Err(format!("连接被拒绝：{host}:{port} 没有服务在监听——请确认服务已启动、端口填写正确"))
            } else if lower.contains("timed out") || lower.contains("timeout") {
                Err(format!("连接超时：{host} 不可达——请检查主机 IP 与网络连通性（防火墙/网段）"))
            } else {
                Err(format!("连接失败：{e}"))
            }
        }
    }
}

/// HTTP GET 探测：返回状态码文本（如 "200"）；非 2xx 视为失败。
fn probe_http_status(host: &str, port: u16, path: &str) -> Result<String, String> {
    use std::io::{Read, Write};
    let mut s = tcp_probe(host, port)?;
    let req = format!(
        "GET {path} HTTP/1.0\r\nHost: {host}\r\nUser-Agent: dfy-ops-remote-test\r\nConnection: close\r\n\r\n"
    );
    s.write_all(req.as_bytes())
        .map_err(|e| format!("发送 HTTP 请求失败：{e}"))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 512];
    while buf.len() < 4096 {
        match s.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let status = head.split_whitespace().nth(1).unwrap_or("").to_string();
    if status.starts_with('2') {
        Ok(status)
    } else if status.is_empty() {
        Err("响应不是 HTTP 协议".into())
    } else {
        Err(format!("HTTP {status}"))
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
