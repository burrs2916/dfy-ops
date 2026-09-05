use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// 解析 host:port 为 SocketAddr（取第一个结果）。
fn resolve(host: &str, port: u16) -> Option<SocketAddr> {
    (host, port).to_socket_addrs().ok()?.next()
}

/// host 是否为本机回环地址/别名。
fn is_loopback_host(host: &str) -> bool {
    host == "localhost" || host == "::1" || host.starts_with("127.")
}

/// TCP 端口探活。host 默认用 127.0.0.1。
///
/// 回环目标双栈探测：dev 服务器（如 `ng serve`）常只绑 IPv6 回环 `[::1]`，
/// 此时 `127.0.0.1` 连接被拒——对回环地址把 127.0.0.1 与 ::1 都纳入候选，
/// 任一栈可达即视为存活。
pub fn check_tcp(host: &str, port: u16, timeout_ms: u64) -> bool {
    let mut addrs: Vec<SocketAddr> = resolve(host, port).into_iter().collect();
    if is_loopback_host(host) {
        for candidate in ["127.0.0.1", "::1"] {
            if let Ok(iter) = (candidate, port).to_socket_addrs() {
                for a in iter {
                    if !addrs.contains(&a) {
                        addrs.push(a);
                    }
                }
            }
        }
    }
    addrs
        .iter()
        .any(|addr| TcpStream::connect_timeout(addr, Duration::from_millis(timeout_ms)).is_ok())
}

/// 从 URL 中解析 host:port（用于 HTTP 健康探测的地址）。
fn url_addr(url: &str) -> Option<(String, u16)> {
    let u = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let hostport = u.split('/').next().unwrap_or(u);
    let (host, port) = hostport.rsplit_once(':')?;
    let port: u16 = port.parse().ok()?;
    Some((host.to_string(), port))
}

/// 简单的 HTTP 健康探测：建立连接并发送 GET，读取首行判断是否 HTTP 正常响应。
pub fn check_http(url: &str, timeout_ms: u64) -> bool {
    use std::io::{Read, Write};
    let Some((host, port)) = url_addr(url) else {
        return false;
    };
    let Some(addr) = resolve(&host, port) else {
        return false;
    };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms)) else {
        return false;
    };
    stream
        .set_read_timeout(Some(Duration::from_millis(timeout_ms)))
        .ok();
    stream
        .set_write_timeout(Some(Duration::from_millis(timeout_ms)))
        .ok();
    if stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut buf = [0u8; 512];
    match stream.read(&mut buf) {
        Ok(n) if n > 0 => {
            let s = String::from_utf8_lossy(&buf[..n]);
            s.starts_with("HTTP/1") && !s.contains(" 503") && !s.contains(" 502")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// 回归：回环探测必须覆盖 IPv6 栈——只绑 `[::1]` 的服务（如 `ng serve`），
    /// 用 127.0.0.1 探测也必须能发现。
    #[test]
    fn check_tcp_detects_v6_only_loopback_listener() {
        let listener = TcpListener::bind("[::1]:0").expect("绑定 [::1]:0");
        let port = listener.local_addr().unwrap().port();
        assert!(
            check_tcp("127.0.0.1", port, 500),
            "只绑 [::1] 的监听应被回环双栈探测发现"
        );
    }

    #[test]
    fn check_tcp_rejects_closed_loopback_port() {
        // 回环 1 端口（tcpmux）在常规开发机上不会有服务
        assert!(!check_tcp("127.0.0.1", 1, 200));
    }
}
