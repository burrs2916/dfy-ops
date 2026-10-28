//! Nginx 反向代理配置可视化运维（conf/nginx.conf 指令级读写）。
//!
//! 背景：nginx.conf 是块嵌套结构，但运维关心的指令（listen / server_name / root /
//! proxy_pass / 日志）在整个文件中都是唯一的——逐行原地替换足够稳，不引入复杂解析器。
//!
//! 可运维字段（与 install.rs build_nginx_conf 模板一一对应）：
//!   - listen_port      监听端口（server 块 listen 指令）
//!   - server_name      主机名（server_name）
//!   - frontend_root    前端静态目录（root）
//!   - backend_host     后端 API 反代主机（proxy_pass 提取）
//!   - backend_port     后端 API 反代端口（proxy_pass 提取）
//!   - access_log       访问日志路径
//!   - error_log        错误日志路径

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 可运维的 nginx.conf 字段。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NginxConfig {
    pub listen_port: String,
    pub server_name: String,
    pub frontend_root: String,
    pub backend_host: String,
    pub backend_port: String,
    pub access_log: String,
    pub error_log: String,
}

/// nginx.conf 路径：`{install_dir}/conf/nginx.conf`。
pub fn config_path(install_dir: &Path) -> PathBuf {
    install_dir.join("conf").join("nginx.conf")
}

/// 从一行指令中提取 (指令名, 原始分隔空格数, 值字符串)。
/// `"listen       80;"` → `("listen", 7, "80")`。
fn parse_directive_with_spacing(line: &str) -> Option<(&str, usize, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let space_pos = trimmed.find(' ')?;
    let name = &trimmed[..space_pos];
    let rest = &trimmed[space_pos..];
    let spacing = rest.chars().take_while(|c| *c == ' ').count();
    let val = rest[spacing..].trim().trim_end_matches(';').to_string();
    if val.is_empty() {
        return None;
    }
    Some((name, spacing, val))
}

/// 从一行指令中提取 "key value" 部分（指令名 + 空格 + 值，忽略末尾分号和多余空格）。
/// 返回 (指令名, 值字符串)。如 `"listen       80;"` → `("listen", "80")`。
fn parse_directive(line: &str) -> Option<(&str, String)> {
    parse_directive_with_spacing(line).map(|(n, _, v)| (n, v))
}

/// 从 proxy_pass URL 提取 host 和 port。
/// `proxy_pass http://127.0.0.1:48085/api/;` → ("127.0.0.1", "48085")
fn parse_proxy_pass(val: &str) -> (String, String) {
    // 去掉前缀 http:// https:// 和末尾路径
    let stripped = val
        .trim()
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let mut parts = stripped.splitn(2, '/');
    let hostport = parts.next().unwrap_or("");
    let mut hp = hostport.rsplitn(2, ':');
    let port = hp.next().unwrap_or("").to_string();
    let host = hp.next().unwrap_or("").to_string();
    (host, port)
}

/// 用 host/port 拼接回 proxy_pass 值：`http://{host}:{port}/api/`。
fn build_proxy_pass(host: &str, port: &str) -> String {
    if host.is_empty() || port.is_empty() {
        return String::new();
    }
    format!("http://{host}:{port}/api/")
}

/// 从 nginx.conf 文本解析运维字段。
pub fn parse(text: &str) -> NginxConfig {
    let mut cfg = NginxConfig::default();
    for line in text.lines() {
        let Some((name, val)) = parse_directive(line) else {
            continue;
        };
        match name {
            "listen" => cfg.listen_port = val,
            "server_name" => cfg.server_name = val,
            "root" => cfg.frontend_root = val,
            "proxy_pass" => {
                let (h, p) = parse_proxy_pass(&val);
                cfg.backend_host = h;
                cfg.backend_port = p;
            }
            "access_log" => cfg.access_log = val,
            "error_log" => cfg.error_log = val,
            _ => {}
        }
    }
    cfg
}

/// 用 new_cfg 的值逐行替换 nginx.conf 中对应指令（保留原有缩进和格式）。
/// 字段为空字符串时不覆盖原值。
pub fn apply(text: &str, new_cfg: &NginxConfig) -> String {
    let mut out = Vec::with_capacity(text.lines().count());
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            out.push(line.to_string());
            continue;
        }
        let Some((name, spacing, _old_val)) = parse_directive_with_spacing(line) else {
            out.push(line.to_string());
            continue;
        };
        let new_val: Option<String> = match name {
            "listen" => {
                if !new_cfg.listen_port.is_empty() { Some(new_cfg.listen_port.clone()) } else { None }
            }
            "server_name" => {
                if !new_cfg.server_name.is_empty() { Some(new_cfg.server_name.clone()) } else { None }
            }
            "root" => {
                if !new_cfg.frontend_root.is_empty() { Some(new_cfg.frontend_root.clone()) } else { None }
            }
            "proxy_pass" => {
                let v = build_proxy_pass(&new_cfg.backend_host, &new_cfg.backend_port);
                if !v.is_empty() { Some(v) } else { None }
            }
            "access_log" => {
                if !new_cfg.access_log.is_empty() { Some(new_cfg.access_log.clone()) } else { None }
            }
            "error_log" => {
                if !new_cfg.error_log.is_empty() { Some(new_cfg.error_log.clone()) } else { None }
            }
            _ => None,
        };
        if let Some(v) = new_val {
            let indent = &line[..line.len() - line.trim_start().len()];
            let spaces = " ".repeat(spacing);
            out.push(format!("{indent}{name}{spaces}{v};"));
        } else {
            out.push(line.to_string());
        }
    }
    let mut result = out.join("\n");
    if !result.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// 读取 nginx.conf；文件不存在返回 None。
pub fn read(install_dir: &Path) -> Result<Option<NginxConfig>, String> {
    let path = config_path(install_dir);
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
    Ok(Some(parse(&text)))
}

/// 原子写回 nginx.conf：临时文件 + rename。
pub fn write(install_dir: &Path, cfg: &NginxConfig) -> Result<PathBuf, String> {
    let path = config_path(install_dir);
    if !path.exists() {
        return Err(format!(
            "nginx.conf 不存在：{}。请先安装 Nginx（资源目录 nginx 包）",
            path.display()
        ));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
    let new_text = apply(&text, cfg);
    let tmp = path.with_extension("conf.tmp");
    std::fs::write(&tmp, &new_text).map_err(|e| format!("写入临时文件失败：{e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("替换 nginx.conf 失败：{e}"))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"worker_processes  1;

events {
    worker_connections  1024;
}

http {
    include       mime.types;
    default_type  application/octet-stream;

    sendfile        on;
    keepalive_timeout  65;

    server {
        listen       80;
        server_name  localhost;

        root  C:/services/dfy-ui-admin-vue3-master/dist;
        index index.html index.htm;

        location / {
            try_files $uri $uri/ /index.html;
        }

        location /api/ {
            proxy_pass http://127.0.0.1:48085/api/;
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
        }

        error_log  C:/services/nginx/logs/error.log;
        access_log C:/services/nginx/logs/access.log;
        pid        C:/services/nginx/logs/nginx.pid;
    }
}
"#;

    #[test]
    fn parse_extracts_all_directives() {
        let c = parse(SAMPLE);
        assert_eq!(c.listen_port, "80");
        assert_eq!(c.server_name, "localhost");
        assert_eq!(c.frontend_root, "C:/services/dfy-ui-admin-vue3-master/dist");
        assert_eq!(c.backend_host, "127.0.0.1");
        assert_eq!(c.backend_port, "48085");
        assert_eq!(c.access_log, "C:/services/nginx/logs/access.log");
        assert_eq!(c.error_log, "C:/services/nginx/logs/error.log");
    }

    #[test]
    fn apply_replaces_and_preserves_format() {
        let new = NginxConfig {
            listen_port: "8080".into(),
            server_name: "dfy.example.com".into(),
            backend_host: "192.168.56.101".into(),
            backend_port: "48085".into(),
            ..Default::default()
        };
        let out = apply(SAMPLE, &new);
        // 修改的指令原地替换
        assert!(out.contains("listen       8080;"), "缩进应保留: {out}");
        assert!(out.contains("server_name  dfy.example.com;"), "{out}");
        assert!(out.contains("proxy_pass http://192.168.56.101:48085/api/;"), "{out}");
        // 未改的指令保持原值
        assert!(out.contains("listen       8080;"));
        assert!(out.contains("root  C:/services/dfy-ui-admin-vue3-master/dist;"));
        assert!(out.contains("access_log C:/services/nginx/logs/access.log;"));
        // 块结构、其他指令完全不动
        assert!(out.contains("try_files $uri $uri/ /index.html;"));
        assert!(out.contains("worker_connections  1024;"));
    }

    #[test]
    fn apply_roundtrip_parse() {
        let new = NginxConfig {
            listen_port: "9090".into(),
            backend_host: "10.0.0.5".into(),
            backend_port: "48086".into(),
            ..Default::default()
        };
        let out = apply(SAMPLE, &new);
        let c = parse(&out);
        assert_eq!(c.listen_port, "9090");
        assert_eq!(c.backend_host, "10.0.0.5");
        assert_eq!(c.backend_port, "48086");
        // 未改字段保持原值
        assert_eq!(c.server_name, "localhost");
    }

    #[test]
    fn empty_field_preserves_original() {
        let new = NginxConfig::default(); // 全部空
        let out = apply(SAMPLE, &new);
        // 所有指令保持不变
        let c = parse(&out);
        assert_eq!(c.listen_port, "80");
        assert_eq!(c.backend_host, "127.0.0.1");
        assert_eq!(c.server_name, "localhost");
    }
}
