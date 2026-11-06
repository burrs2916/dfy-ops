//! edge-conductor（config.toml）在线编辑——配置中心「边缘计算」分区。
//!
//! 与 nginx / java 的 external-config 同一模式：运维字段行级替换、保留
//! 注释与未知行。TOML 必须按段定位——[websocket] 与 [http] 同名 port、
//! [greptimedb]/[VerneMQ] 各有地址键，逐行跟踪当前段再匹配键，防止改错行。
//! 保存前整体 TOML 语法校验 + 原文件备份，改坏可回滚。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 可运维字段（只暴露换 IP/端口的关键少数，其余段落原样透传）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EdgeConfig {
    /// HTTP API 监听地址
    #[serde(rename = "httpBind")]
    pub http_bind: String,
    /// HTTP API 监听端口
    #[serde(rename = "httpPort")]
    pub http_port: String,
    /// WebSocket 监听地址
    #[serde(rename = "wsBind")]
    pub ws_bind: String,
    /// WebSocket 监听端口
    #[serde(rename = "wsPort")]
    pub ws_port: String,
    /// 上游 DFY 后端地址（http://host:port——换 IP/端口的主要场景）
    #[serde(rename = "upstreamServerUrl")]
    pub upstream_server_url: String,
    /// GreptimeDB 时序库地址（host:port）
    #[serde(rename = "greptimeAddr")]
    pub greptime_addr: String,
    /// MQTT Broker 地址（VerneMQ）
    #[serde(rename = "mqttBrokerUrl")]
    pub mqtt_broker_url: String,
    /// MQTT Broker 端口
    #[serde(rename = "mqttBrokerPort")]
    pub mqtt_broker_port: String,
}

/// config.toml 路径：安装根（与 registry install_dir / registry.logPaths 同源）
pub fn config_path(install_dir: &Path) -> PathBuf {
    install_dir.join("config.toml")
}

/// (TOML 段, 键, struct 字段)。段名原样匹配（[VerneMQ] 大小写敏感，与
/// edge-conductor 侧 serde(rename = "VerneMQ") 一致）
const FIELDS: [(&str, &str, &str); 8] = [
    ("http", "bind", "http_bind"),
    ("http", "port", "http_port"),
    ("websocket", "bind", "ws_bind"),
    ("websocket", "port", "ws_port"),
    ("upstream.external", "upstream_server_url", "upstream_server_url"),
    ("greptimedb", "addr", "greptime_addr"),
    ("VerneMQ", "broker_url", "mqtt_broker_url"),
    ("VerneMQ", "broker_port", "mqtt_broker_port"),
];

/// 提取标量值：去行尾注释与引号（托管字段都是标量，数组/多行结构不在范围）
fn scalar_value(raw_val: &str) -> String {
    let v = raw_val.split('#').next().unwrap_or(raw_val).trim();
    v.trim_matches('"').trim_matches('\'').to_string()
}

/// 逐行解析：跟踪当前 [段]，段+键命中 → 填字段
pub fn parse_fields(raw: &str) -> EdgeConfig {
    let mut cfg = EdgeConfig::default();
    let mut section = String::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.starts_with('[') && t.ends_with(']') {
            section = t[1..t.len() - 1].trim().to_string();
            continue;
        }
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(eq) = t.find('=') {
            let key = t[..eq].trim();
            let val = scalar_value(&t[eq + 1..]);
            if let Some(field) = FIELDS
                .iter()
                .find(|(sec, k, _)| *sec == section && *k == key)
                .map(|(_, _, f)| *f)
            {
                match field {
                    "http_bind" => cfg.http_bind = val,
                    "http_port" => cfg.http_port = val,
                    "ws_bind" => cfg.ws_bind = val,
                    "ws_port" => cfg.ws_port = val,
                    "upstream_server_url" => cfg.upstream_server_url = val,
                    "greptime_addr" => cfg.greptime_addr = val,
                    "mqtt_broker_url" => cfg.mqtt_broker_url = val,
                    "mqtt_broker_port" => cfg.mqtt_broker_port = val,
                    _ => {}
                }
            }
        }
    }
    cfg
}

/// 保存前校验：端口 1-65535、地址非空、上游必须 http(s) URL
fn validate(cfg: &EdgeConfig) -> Result<(), String> {
    for (label, v) in [
        ("HTTP 端口", &cfg.http_port),
        ("WebSocket 端口", &cfg.ws_port),
        ("MQTT 端口", &cfg.mqtt_broker_port),
    ] {
        let p: u16 = v
            .trim()
            .parse()
            .map_err(|_| format!("{label} 不是合法端口号：{v:?}"))?;
        if p == 0 {
            return Err(format!("{label} 不能为 0"));
        }
    }
    if cfg.http_bind.trim().is_empty() {
        return Err("HTTP 监听地址不能为空".into());
    }
    if cfg.ws_bind.trim().is_empty() {
        return Err("WebSocket 监听地址不能为空".into());
    }
    let url = cfg.upstream_server_url.trim();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("上游服务地址需以 http:// 或 https:// 开头".into());
    }
    if cfg.greptime_addr.trim().is_empty() {
        return Err("GreptimeDB 地址不能为空".into());
    }
    if cfg.mqtt_broker_url.trim().is_empty() {
        return Err("MQTT Broker 地址不能为空".into());
    }
    Ok(())
}

/// 原地替换：段+键命中 → 替换 = 右侧值（保持引号风格与行尾注释）
fn apply_fields(raw: &str, cfg: &EdgeConfig) -> String {
    let mut section = String::new();
    let mut out = String::with_capacity(raw.len() + 256);
    for line in raw.lines() {
        let t = line.trim_start();
        let trimmed = t.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            section = trimmed[1..trimmed.len() - 1].trim().to_string();
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let mut replaced = false;
        if !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some(eq) = trimmed.find('=') {
                let key = trimmed[..eq].trim();
                if let Some(field) = FIELDS
                    .iter()
                    .find(|(sec, k, _)| *sec == section && *k == key)
                    .map(|(_, _, f)| *f)
                {
                    let new_val: &str = match field {
                        "http_bind" => &cfg.http_bind,
                        "http_port" => &cfg.http_port,
                        "ws_bind" => &cfg.ws_bind,
                        "ws_port" => &cfg.ws_port,
                        "upstream_server_url" => &cfg.upstream_server_url,
                        "greptime_addr" => &cfg.greptime_addr,
                        "mqtt_broker_url" => &cfg.mqtt_broker_url,
                        "mqtt_broker_port" => &cfg.mqtt_broker_port,
                        _ => "",
                    };
                    let rhs = &trimmed[eq + 1..];
                    // 行尾注释拼回（标量值不含 #，rfind 即注释起点）
                    let comment = match rhs.rfind('#') {
                        Some(i) => format!("  {}", rhs[i..].trim_end()),
                        None => String::new(),
                    };
                    // 引号风格跟随原行（字符串带引号，端口裸数字）
                    let orig = rhs.split('#').next().unwrap_or(rhs).trim();
                    let nv = new_val.trim();
                    let nv = if orig.starts_with('"') {
                        format!("\"{nv}\"")
                    } else {
                        nv.to_string()
                    };
                    let indent = &line[..line.len() - t.len()];
                    out.push_str(&format!("{indent}{key} = {nv}{comment}\n"));
                    replaced = true;
                }
            }
        }
        if !replaced {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// 读配置（文件不存在 → None）
pub fn read(install_dir: &Path) -> Result<Option<EdgeConfig>, String> {
    let p = config_path(install_dir);
    if !p.is_file() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&p).map_err(|e| format!("{}：{e}", p.display()))?;
    Ok(Some(parse_fields(&raw)))
}

/// 保存：校验 → 整体 TOML 语法校验 → 备份 → 原子写回。
/// 备份名 config.toml.bak-<epoch 秒>（.log 判定不受影响；回滚手动改名即可）
pub fn save(install_dir: &Path, cfg: &EdgeConfig) -> Result<(), String> {
    validate(cfg)?;
    let p = config_path(install_dir);
    if !p.is_file() {
        return Err(format!(
            "config.toml 不存在：{}（请先安装边缘计算）",
            p.display()
        ));
    }
    let raw = std::fs::read_to_string(&p).map_err(|e| format!("读取 {} 失败：{e}", p.display()))?;
    let new_raw = apply_fields(&raw, cfg);
    // 行级替换产出非法 TOML 的兜底（重复键/坏引号）——不写盘，直接报错
    toml::from_str::<toml::Value>(&new_raw)
        .map_err(|e| format!("修改后的 config.toml 无法解析（未写盘）：{e}"))?;
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let bak = p.with_extension(format!("toml.bak-{epoch}"));
    std::fs::copy(&p, &bak).map_err(|e| format!("备份 {} 失败：{e}", bak.display()))?;
    let tmp = p.with_extension("toml.new");
    std::fs::write(&tmp, &new_raw).map_err(|e| format!("写入临时文件失败：{e}"))?;
    std::fs::rename(&tmp, &p).map_err(|e| format!("替换 {} 失败：{e}", p.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小样例：覆盖段定位关键场景——[http]/[websocket] 同名 port、
    /// [VerneMQ] 带行尾注释、[sqlite.*] 嵌套段干扰
    const SAMPLE: &str = r#"[http]
bind = "127.0.0.1"
port = 8084

[websocket]
bind = "127.0.0.1"
port = 8085

[sqlite]
[sqlite.cache]
db_path = "data/sqlite/cache.db"

[VerneMQ]
broker_url = "127.0.0.1"
broker_port = 18830  # Broker 端口
"#;

    #[test]
    fn parse_distinguishes_same_key_in_different_sections() {
        let cfg = parse_fields(SAMPLE);
        assert_eq!(cfg.http_port, "8084");
        assert_eq!(cfg.ws_port, "8085");
        assert_eq!(cfg.mqtt_broker_port, "18830");
        assert_eq!(cfg.http_bind, "127.0.0.1");
    }

    #[test]
    fn apply_keeps_comments_and_quote_style() {
        let cfg = EdgeConfig {
            http_port: "9084".into(),
            mqtt_broker_port: "18831".into(),
            ..Default::default()
        };
        let out = apply_fields(SAMPLE, &cfg);
        assert!(out.contains("port = 9084"));
        // VerneMQ 端口裸数字保持不带引号 + 行尾注释保留
        assert!(out.contains("broker_port = 18831  # Broker 端口"));
        // 未托管行原样保留
        assert!(out.contains("db_path = \"data/sqlite/cache.db\""));
        // 引号风格：bind 字符串保持带引号
        let cfg2 = EdgeConfig {
            http_bind: "0.0.0.0".into(),
            ..Default::default()
        };
        assert!(apply_fields(SAMPLE, &cfg2).contains("bind = \"0.0.0.0\""));
    }

    #[test]
    fn validate_rejects_bad_input() {
        let mut cfg = parse_fields(SAMPLE);
        // SAMPLE 未含 upstream.external 段，补合法值让基线通过
        cfg.upstream_server_url = "http://127.0.0.1:48085".into();
        cfg.greptime_addr = "127.0.0.1:4003".into();
        cfg.mqtt_broker_url = "127.0.0.1".into();
        assert!(validate(&cfg).is_ok());
        cfg.http_port = "abc".into();
        assert!(validate(&cfg).is_err());
        cfg.http_port = "8084".into();
        cfg.upstream_server_url = "localhost:48085".into();
        assert!(validate(&cfg).is_err(), "上游地址必须 http(s):// 开头");
    }
}
