use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// 解析 host:port 为 SocketAddr（取第一个结果）。
fn resolve(host: &str, port: u16) -> Option<SocketAddr> {
    (host, port).to_socket_addrs().ok()?.next()
}

/// TCP 端口探活。host 默认用 127.0.0.1。
pub fn check_tcp(host: &str, port: u16, timeout_ms: u64) -> bool {
    let Some(addr) = resolve(host, port) else {
        return false;
    };
    match TcpStream::connect_timeout(&addr, Duration::from_millis(timeout_ms)) {
        Ok(_) => true,
        Err(_) => false,
    }
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
