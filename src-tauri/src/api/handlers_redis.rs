//! Redis 远程运维：平台不要求 Redis 本地安装，通过 Rust 原生 redis 客户端
//! （RESP 协议 TCP 直连，纯 Rust 零外部依赖——不再需要交付包携带 redis-cli.exe），
//! 提供「类 Another Redis Desktop Manager」的管理能力——总览信息、SCAN 遍历、
//! 多类型 Key 详情、删除与 TTL 管理。连接凭据托管在 config/redis_credential.json
//! （默认 192.168.56.101:6379，生产环境在界面修改）。
//!
//! 链路设计：同步客户端 + blocking 线程池。管理页操作是低频人工动作，每请求
//! 短连接（连接 3s / 读写 5s 超时）——局域网握手 <1ms，且用户随时可能改凭据，
//! 缓存长连接反而引入失效复杂度。值以字节读取（二进制安全），非 UTF-8 显示
//! 占位符；错误按类别映射中文提示（密码错 / 连接拒绝 / 超时分得清）。

use axum::extract::{Query, State};
use axum::Json;
use redis::IntoConnectionInfo;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use std::sync::Arc;

use super::handlers_repair::blocking;
use super::server::AppState;
use crate::ops::redis_credential::{self, RedisCredential};

/// SCAN 全量迭代后最多返回的 key 数（超过则截断并提示细化模式）。
const MAX_KEYS_RETURNED: usize = 2000;
/// SCAN 全量收集的硬上限（防 pattern 过宽时空转/内存失控）。
const MAX_KEYS_SCANNED: usize = 500_000;
/// 单个 value 的展示上限（超长截断，防止 UI 卡顿）。
const MAX_VALUE_CHARS: usize = 65536;

/// 建立到托管凭据端点的连接：connect 3s / 读写 5s 超时，有密码时连接握手后
/// 自动 AUTH。不可达主机不会把接口挂到 OS 级 TCP 超时（Windows 上 20s+）。
fn open_conn(cred: &RedisCredential) -> Result<redis::Connection, String> {
    let mut redis_info = redis::RedisConnectionInfo::default().set_db(0);
    // 空密码不发 AUTH——无密码实例收到 AUTH 会直接报错
    if !cred.password.is_empty() {
        redis_info = redis_info.set_password(&cred.password);
    }
    let info = redis::ConnectionAddr::Tcp(cred.host.clone(), cred.port)
        .into_connection_info()
        .map_err(|e| format!("连接配置无效：{e}"))?
        .set_redis_settings(redis_info);
    let client = redis::Client::open(info).map_err(|e| format!("连接配置无效：{e}"))?;
    let conn = client
        .get_connection_with_timeout(std::time::Duration::from_secs(3))
        .map_err(conn_err)?;
    let _ = conn.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let _ = conn.set_write_timeout(Some(std::time::Duration::from_secs(5)));
    Ok(conn)
}

/// 连接/命令错误 → 面向用户的中文提示：按错误类别给出「该去改哪里」的指引，
/// 密码错 / 连接拒绝 / 超时 / 解析失败不再混为一谈。
fn conn_err(e: redis::RedisError) -> String {
    let msg = e.to_string();
    match e.kind() {
        redis::ErrorKind::AuthenticationFailed => {
            "认证失败：密码不正确——请到「连接配置」检查密码设置".into()
        }
        redis::ErrorKind::Io => {
            let lower = msg.to_ascii_lowercase();
            if lower.contains("refused") {
                "连接被拒绝：目标地址没有 Redis 在监听——请到「连接配置」检查主机/端口，并确认远程 Redis 已启动".into()
            } else if lower.contains("timed out") || lower.contains("timeout") {
                "连接超时：主机不可达（网络不通或防火墙拦截）——请到「连接配置」检查主机地址".into()
            } else if lower.contains("resolve") || lower.contains("lookup") || lower.contains("name or service") {
                "主机名无法解析——请到「连接配置」检查主机地址".into()
            } else {
                format!("网络错误：{msg}")
            }
        }
        _ if msg.contains("NOAUTH") => {
            "该 Redis 需要密码认证——请到「连接配置」填写密码".into()
        }
        _ => format!("Redis 操作失败：{msg}"),
    }
}

/// INFO 输出解析：跳过 `# Section` 与空行，key:value 逐行。
fn parse_info(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
    .collect()
}

/// keyspace 节聚合：db0:keys=10,expires=1,avg_ttl=0 → (总 keys, 总 expires)。
fn aggregate_keyspace(text: &str) -> (u64, u64) {
    let mut keys = 0u64;
    let mut expires = 0u64;
    for (k, v) in parse_info(text) {
        if !k.starts_with("db") || !k[2..].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        for part in v.split(',') {
            if let Some(n) = part.strip_prefix("keys=") {
                keys += n.parse::<u64>().unwrap_or(0);
            } else if let Some(n) = part.strip_prefix("expires=") {
                expires += n.parse::<u64>().unwrap_or(0);
            }
        }
    }
    (keys, expires)
}

fn state_dir(state: &AppState) -> std::path::PathBuf {
    state.config_dir.clone()
}

/// 字节值 → 展示文本（二进制安全）：UTF-8 按文本截断展示，非 UTF-8 显示占位符，
/// 不再有 redis-cli 文本管道下的乱码/换行错位问题。
fn display_bytes(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => value_of(s),
        Err(_) => format!("<binary {} 字节，非 UTF-8 文本>", b.len()),
    }
}

// ===== 接口 =====

/// 当前托管凭据（本机信任边界内明文返回，供连接配置对话框回显）。
pub async fn credential(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let cred = redis_credential::load(&state_dir(&state));
    Json(json!({ "host": cred.host, "port": cred.port, "password": cred.password }))
}

#[derive(Deserialize)]
pub struct SaveCredentialReq {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub password: String,
}

/// 保存托管凭据（连接配置对话框）。只写托管文件，不主动探测。
pub async fn save_credential(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SaveCredentialReq>,
) -> Json<serde_json::Value> {
    let host = req.host.trim().to_string();
    if host.is_empty() {
        return Json(json!({ "ok": false, "error": "主机地址不能为空" }));
    }
    let cred = RedisCredential {
        host,
        port: req.port,
        password: req.password,
    };
    match redis_credential::save(&state_dir(&state), &cred) {
        Ok(()) => Json(json!({ "ok": true })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Deserialize)]
pub struct TestReq {
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub password: String,
}

/// 测试连接（用表单当前值，不落盘）：PING 校验 + 顺带取版本。
pub async fn test(Json(req): Json<TestReq>) -> Json<serde_json::Value> {
    let cred = RedisCredential {
        host: req.host.trim().to_string(),
        port: req.port,
        password: req.password,
    };
    let res = blocking(move || -> Result<String, String> {
        let mut conn = open_conn(&cred)?;
        let pong: String = redis::cmd("PING").query(&mut conn).map_err(conn_err)?;
        if pong != "PONG" {
            return Err(format!("PING 未返回 PONG（收到：{pong}）"));
        }
        let info: String = redis::cmd("INFO")
            .arg("server")
            .query(&mut conn)
            .unwrap_or_default();
        let version = parse_info(&info)
            .into_iter()
            .find(|(k, _)| k == "redis_version")
            .map(|(_, v)| v)
            .unwrap_or_default();
        Ok(version)
    })
    .await;
    match res {
        Ok(version) => Json(json!({ "ok": true, "version": version })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

/// 总览：INFO server/memory/clients + keyspace 聚合。
pub async fn overview(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let cred = redis_credential::load(&state_dir(&state));
    let res = blocking(move || -> Result<serde_json::Value, String> {
        let mut conn = open_conn(&cred)?;
        let all: String = redis::cmd("INFO").query(&mut conn).map_err(conn_err)?;
        let kv = parse_info(&all);
        let get = |key: &str| kv.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        let (total_keys, total_expires) = aggregate_keyspace(&all);
        Ok(json!({
            "version": get("redis_version").unwrap_or_default(),
            "mode": get("redis_mode").unwrap_or_else(|| "standalone".into()),
            "os": get("os").unwrap_or_default(),
            "uptimeDays": get("uptime_days").unwrap_or_default(),
            "usedMemory": get("used_memory_human").unwrap_or_default(),
            "usedMemoryPeak": get("used_memory_peak_human").unwrap_or_default(),
            "maxMemory": get("maxmemory_human").unwrap_or_default(),
            "connectedClients": get("connected_clients").unwrap_or_default(),
            "blockedClients": get("blocked_clients").unwrap_or_default(),
            "totalConnections": get("total_connections_received").unwrap_or_default(),
            "totalCommands": get("total_commands_processed").unwrap_or_default(),
            "aofEnabled": get("aof_enabled").unwrap_or_default(),
            "rdbLastStatus": get("rdb_last_bgsave_status").unwrap_or_default(),
            "totalKeys": total_keys,
            "totalExpires": total_expires,
        }))
    })
    .await;
    match res {
        Ok(v) => Json(v),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Deserialize)]
pub struct KeysQuery {
    #[serde(default = "default_pattern")]
    pub pattern: String,
}

fn default_pattern() -> String {
    "*".into()
}

/// SCAN 游标遍历（不用 KEYS）：按模式筛选，首见去重，全量计数 + 截断返回。
pub async fn keys(
    State(state): State<Arc<AppState>>,
    Query(q): Query<KeysQuery>,
) -> Json<serde_json::Value> {
    let cred = redis_credential::load(&state_dir(&state));
    let pattern = q.pattern.trim().to_string();
    let pattern = if pattern.is_empty() { "*".to_string() } else { pattern };
    let res = blocking(move || -> Result<serde_json::Value, String> {
        let mut conn = open_conn(&cred)?;
        let mut cursor: u64 = 0;
        let mut seen: HashSet<String> = HashSet::new();
        let mut all: Vec<String> = Vec::new();
        loop {
            let (next, batch): (u64, Vec<String>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("MATCH")
                .arg(&pattern)
                .arg("COUNT")
                .arg(500)
                .query(&mut conn)
                .map_err(conn_err)?;
            for k in batch {
                // SCAN 扩容期可能重复返回同一 key：首见去重，total 更真实
                if seen.insert(k.clone()) {
                    all.push(k);
                }
            }
            cursor = next;
            if cursor == 0 || all.len() >= MAX_KEYS_SCANNED {
                break;
            }
        }
        let total = all.len();
        let truncated = total > MAX_KEYS_RETURNED;
        // 对展示的 key 批量 pipeline TYPE（一次网络往返）：列表行直接带类型徽标，
        // 用户不必逐个点开才知道类型
        let shown_len = total.min(MAX_KEYS_RETURNED);
        let mut pipe = redis::pipe();
        for k in &all[..shown_len] {
            pipe.cmd("TYPE").arg(k);
        }
        let kinds: Vec<String> = if shown_len > 0 {
            pipe.query(&mut conn).map_err(conn_err)?
        } else {
            vec![]
        };
        let items: Vec<serde_json::Value> = all[..shown_len]
            .iter()
            .zip(kinds)
            .map(|(name, kind)| json!({ "name": name, "kind": kind }))
            .collect();
        Ok(json!({
            "keys": items,
            "total": total,
            "truncated": truncated,
        }))
    })
    .await;
    match res {
        Ok(v) => Json(v),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Deserialize)]
pub struct KeyQuery {
    pub name: String,
}

fn value_of(raw: &str) -> String {
    let raw = raw.trim_end_matches('\r').trim_end_matches('\n');
    if raw.chars().count() > MAX_VALUE_CHARS {
        let head: String = raw.chars().take(MAX_VALUE_CHARS).collect();
        format!("{head}\n…（已截断，共 {} 字符）", raw.chars().count())
    } else {
        raw.to_string()
    }
}

/// Key 详情：TYPE/TTL/MEMORY USAGE + 按类型取内容（同一条连接内完成，
/// 原先 4-8 个 redis-cli 进程的调用合并为几条命令）。
/// rows 统一为 [标签, 值] 二元组：string → (值, 内容)，hash → (field, value)，
/// list → (序号, 元素)，set → (成员, 成员)，zset → (member, score)。
pub async fn key_detail(
    State(state): State<Arc<AppState>>,
    Query(q): Query<KeyQuery>,
) -> Json<serde_json::Value> {
    let cred = redis_credential::load(&state_dir(&state));
    let name = q.name.clone();
    let res = blocking(move || -> Result<serde_json::Value, String> {
        let mut conn = open_conn(&cred)?;
        let kind: String = redis::cmd("TYPE")
            .arg(&name)
            .query(&mut conn)
            .map_err(conn_err)?;
        if kind == "none" {
            return Err("Key 不存在（可能已被删除或过期）".to_string());
        }
        let ttl: i64 = redis::cmd("TTL")
            .arg(&name)
            .query::<i64>(&mut conn)
            .unwrap_or(-3);
        // MEMORY USAGE 在部分老版本/代理上不支持：容忍失败，展示为未知
        let memory: Option<u64> = redis::cmd("MEMORY")
            .arg("USAGE")
            .arg(&name)
            .query::<Option<u64>>(&mut conn)
            .unwrap_or(None);

        let (total, rows): (usize, Vec<Vec<String>>) = match kind.as_str() {
            "string" => {
                let v: Option<Vec<u8>> = redis::cmd("GET")
                    .arg(&name)
                    .query(&mut conn)
                    .map_err(conn_err)?;
                let text = v.map(|b| display_bytes(&b)).unwrap_or_else(|| "<nil>".into());
                (1, vec![vec!["值".into(), text]])
            }
            "hash" => {
                let flat: Vec<Vec<u8>> = redis::cmd("HGETALL")
                    .arg(&name)
                    .query(&mut conn)
                    .map_err(conn_err)?;
                let rows: Vec<Vec<String>> = flat
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| vec![display_bytes(&c[0]), display_bytes(&c[1])])
                    .collect();
                (rows.len(), rows)
            }
            "list" => {
                let len: usize = redis::cmd("LLEN")
                    .arg(&name)
                    .query(&mut conn)
                    .map_err(conn_err)?;
                let take = len.min(200);
                let items: Vec<Vec<u8>> = if take == 0 {
                    vec![]
                } else {
                    redis::cmd("LRANGE")
                        .arg(&name)
                        .arg(0)
                        .arg(take - 1)
                        .query(&mut conn)
                        .map_err(conn_err)?
                };
                let rows: Vec<Vec<String>> = items
                    .iter()
                    .enumerate()
                    .map(|(i, b)| vec![format!("[{i}]"), display_bytes(b)])
                    .collect();
                (len, rows)
            }
            "set" => {
                let len: usize = redis::cmd("SCARD")
                    .arg(&name)
                    .query(&mut conn)
                    .map_err(conn_err)?;
                let members: Vec<Vec<u8>> = redis::cmd("SMEMBERS")
                    .arg(&name)
                    .query(&mut conn)
                    .map_err(conn_err)?;
                let rows: Vec<Vec<String>> = members
                    .iter()
                    .take(len.min(200))
                    .map(|b| vec!["成员".into(), display_bytes(b)])
                    .collect();
                (len, rows)
            }
            "zset" => {
                let len: usize = redis::cmd("ZCARD")
                    .arg(&name)
                    .query(&mut conn)
                    .map_err(conn_err)?;
                let flat: Vec<Vec<u8>> = redis::cmd("ZRANGE")
                    .arg(&name)
                    .arg(0)
                    .arg(199)
                    .arg("WITHSCORES")
                    .query(&mut conn)
                    .map_err(conn_err)?;
                let rows: Vec<Vec<String>> = flat
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| vec![display_bytes(&c[0]), display_bytes(&c[1])])
                    .collect();
                (len, rows)
            }
            other => {
                // stream 等扩展类型：给出类型提示，不解析内容
                (0, vec![vec!["类型".into(), format!("{other}（暂不支持内容解析）")]])
            }
        };
        Ok(json!({
            "name": name,
            "kind": kind,
            "ttl": ttl,
            "memory": memory,
            "total": total,
            "rows": rows,
        }))
    })
    .await;
    match res {
        Ok(v) => Json(v),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Deserialize)]
pub struct DeleteReq {
    pub name: String,
}

/// 删除 Key（DEL）。前端负责强确认。
pub async fn delete_key(
    State(state): State<Arc<AppState>>,
    Json(req): Json<DeleteReq>,
) -> Json<serde_json::Value> {
    let cred = redis_credential::load(&state_dir(&state));
    let name = req.name;
    let res = blocking(move || -> Result<u64, String> {
        let mut conn = open_conn(&cred)?;
        let n: u64 = redis::cmd("DEL").arg(&name).query(&mut conn).map_err(conn_err)?;
        if n == 0 {
            Err("删除数为 0（Key 可能已不存在）".to_string())
        } else {
            Ok(n)
        }
    })
    .await;
    match res {
        Ok(n) => Json(json!({ "ok": true, "deleted": n })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[derive(Deserialize)]
pub struct SetTtlReq {
    pub name: String,
    /// -1 → PERSIST（取消过期）；其余为 EXPIRE 秒数（须 > 0）。
    pub seconds: i64,
}

pub async fn set_ttl(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetTtlReq>,
) -> Json<serde_json::Value> {
    let cred = redis_credential::load(&state_dir(&state));
    let name = req.name;
    let seconds = req.seconds;
    let res = blocking(move || -> Result<String, String> {
        let mut conn = open_conn(&cred)?;
        let n: u64 = if seconds == -1 {
            redis::cmd("PERSIST").arg(&name).query(&mut conn).map_err(conn_err)?
        } else if seconds > 0 {
            redis::cmd("EXPIRE").arg(&name).arg(seconds).query(&mut conn).map_err(conn_err)?
        } else {
            return Err("秒数必须为正整数，或 -1 表示永久".to_string());
        };
        if n == 0 {
            Err(if seconds == -1 {
                "PERSIST 未生效（Key 不存在或本就无过期时间）".to_string()
            } else {
                "EXPIRE 未生效（Key 不存在）".to_string()
            })
        } else if seconds == -1 {
            Ok("已取消过期时间（永久保留）".to_string())
        } else {
            Ok(format!("已设置 {seconds} 秒后过期"))
        }
    })
    .await;
    match res {
        Ok(msg) => Json(json!({ "ok": true, "message": msg })),
        Err(e) => Json(json!({ "ok": false, "error": e })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_info_skips_sections_and_empty() {
        let text = "# Server\r\nredis_version:7.2.4\r\n\r\nredis_mode:standalone\r\n# Memory\r\nused_memory_human:1.2M\r\n";
        let kv = parse_info(text);
        assert_eq!(kv.len(), 3);
        assert_eq!(kv[0], ("redis_version".into(), "7.2.4".into()));
        assert_eq!(kv[2], ("used_memory_human".into(), "1.2M".into()));
    }

    #[test]
    fn keyspace_aggregates_all_dbs() {
        let text = "# Keyspace\r\ndb0:keys=10,expires=1,avg_ttl=0\r\ndb1:keys=32,expires=30,avg_ttl=5\r\nnotdb:keys=99\r\n";
        assert_eq!(aggregate_keyspace(text), (42, 31));
    }

    #[test]
    fn keyspace_ignores_non_numeric_db() {
        assert_eq!(aggregate_keyspace("dbX:keys=5\n"), (0, 0));
    }

    #[test]
    fn value_of_truncates_long_values() {
        let long = "x".repeat(70_000);
        let v = value_of(&long);
        assert!(v.contains("已截断"));
        assert!(v.chars().count() < MAX_VALUE_CHARS + 60);
    }

    /// 二进制安全：非 UTF-8 字节不再乱码/破坏行解析，而是显示占位符。
    #[test]
    fn display_bytes_marks_non_utf8() {
        assert_eq!(display_bytes(&[0xFF, 0xFE, 0x00]), "<binary 3 字节，非 UTF-8 文本>");
        assert_eq!(display_bytes("中文\n多行".as_bytes()), "中文\n多行");
    }

    /// 错误映射：认证失败 / 连接拒绝 / 超时 → 各自的中文指引，均指向「连接配置」。
    #[test]
    fn conn_err_maps_categories_to_hints() {
        let auth = redis::RedisError::from((redis::ErrorKind::AuthenticationFailed, "WRONGPASS"));
        assert!(conn_err(auth).contains("连接配置"));

        let refused = redis::RedisError::from((
            redis::ErrorKind::Io,
            "Connection refused (os error 111)",
        ));
        let hint = conn_err(refused);
        assert!(hint.contains("连接被拒绝"), "{hint}");
        assert!(hint.contains("连接配置"), "{hint}");

        let timeout = redis::RedisError::from((redis::ErrorKind::Io, "connection timed out"));
        assert!(conn_err(timeout).contains("连接超时"));
    }

    /// 不可达端点：连接在 3s 超时内失败并映射为中文提示（不挂 OS 级 TCP 超时）。
    #[test]
    fn open_conn_reports_friendly_error_on_refused() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let cred = RedisCredential {
            host: "127.0.0.1".into(),
            port,
            password: String::new(),
        };
        let err = match open_conn(&cred) {
            Ok(_) => panic!("无服务的端口应连接失败"),
            Err(e) => e,
        };
        assert!(err.contains("连接配置"), "错误应指引去连接配置: {err}");
    }
}
