//! Redis 远程运维：平台不要求 Redis 本地安装，通过 redis-cli 连接远程实例，
//! 提供「类 Another Redis Desktop Manager」的管理能力——总览信息、SCAN 遍历、
//! 多类型 Key 详情、删除与 TTL 管理。连接凭据托管在 config/redis_credential.json
//! （默认 192.168.56.101:6379，生产环境在界面修改）。
//!
//! 链路设计：全部经 redis-cli.exe 子进程执行（与 MySQL 运维同链路，不引入
//! Redis 驱动依赖），密码经 REDISCLI_AUTH 环境变量传递，不出现在命令行。
//! redis-cli 在非 TTY（管道）下输出 raw 模式：nil 为空串、值不带引号。

use axum::extract::{Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use super::handlers_repair::{blocking, registry_install_dir};
use super::server::AppState;
use crate::ops::redis_credential::{self, RedisCredential};

/// SCAN 全量迭代后最多返回的 key 数（超过则截断并提示细化模式）。
const MAX_KEYS_RETURNED: usize = 2000;

/// 单个 value 的展示上限（超长截断，防止 UI 卡顿）。
const MAX_VALUE_CHARS: usize = 65536;

/// redis-cli 定位：平台本地安装的 Redis → 交付包自带 resource\redis → PATH。
fn redis_client(state: &AppState) -> Result<String, String> {
    let mut candidates: Vec<String> = Vec::new();
    let dir = registry_install_dir(state, "redis");
    candidates.push(format!("{dir}\\redis-cli.exe"));
    candidates.push(format!("{dir}\\bin\\redis-cli.exe"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(format!("{}\\resource\\redis\\redis-cli.exe", dir.display()));
            candidates.push(format!("{}\\resource\\redis-cli.exe", dir.display()));
        }
    }
    for c in &candidates {
        if std::path::Path::new(c).is_file() {
            return Ok(c.clone());
        }
    }
    // PATH 候选：实跑 --version 确认可用（避免只存在同名文件的情况）
    if let Ok(out) = std::process::Command::new("redis-cli").arg("--version").output() {
        if out.status.success() {
            return Ok("redis-cli".into());
        }
    }
    Err(format!(
        "未找到 redis-cli 客户端（已尝试：{}），请确认平台安装包包含 resource\\redis 目录，或在安装与维护页安装 Redis",
        candidates.join("、")
    ))
}

/// redis-cli 非交互执行：raw 输出。命令级失败（WRONGPASS/ERR 回复）在
/// redis-cli 下 exit code 仍为 0，因此额外检查 (error) 前缀。
fn run_cli(client: &str, cred: &RedisCredential, args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new(client)
        .args(["-h", &cred.host, "-p", &cred.port.to_string()])
        .args(args)
        .env("REDISCLI_AUTH", &cred.password)
        .output()
        .map_err(|e| format!("redis-cli 启动失败：{e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let err_line = |text: &str| -> Option<String> {
        text.lines()
            .filter(|l| l.contains("(error)") || l.contains("Could not connect") || l.contains("WRONGPASS"))
            .last()
            .map(|l| l.trim().to_string())
    };
    if let Some(e) = err_line(&stdout).or_else(|| err_line(&stderr)) {
        return Err(e);
    }
    if !out.status.success() {
        let tail = stderr
            .lines()
            .filter(|l| !l.trim().is_empty())
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" | ");
        return Err(if tail.is_empty() {
            format!("redis-cli 退出码 {:?}", out.status.code())
        } else {
            tail
        });
    }
    Ok(stdout)
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
pub async fn test(State(state): State<Arc<AppState>>, Json(req): Json<TestReq>) -> Json<serde_json::Value> {
    let client = match redis_client(&state) {
        Ok(c) => c,
        Err(e) => return Json(json!({ "ok": false, "error": e })),
    };
    let cred = RedisCredential {
        host: req.host.trim().to_string(),
        port: req.port,
        password: req.password,
    };
    let client2 = client.clone();
    let res = blocking(move || -> Result<String, String> {
        let pong = run_cli(&client2, &cred, &["PING"])?;
        if pong.trim() != "PONG" {
            return Err(format!("PING 未返回 PONG（收到：{}）", pong.trim()));
        }
        let info = run_cli(&client, &cred, &["INFO", "server"]).unwrap_or_default();
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
    let client = match redis_client(&state) {
        Ok(c) => c,
        Err(e) => return Json(json!({ "ok": false, "error": e })),
    };
    let cred = redis_credential::load(&state_dir(&state));
    let res = blocking(move || -> Result<serde_json::Value, String> {
        let all = run_cli(&client, &cred, &["INFO"])?;
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

/// SCAN 遍历（--scan，不用 KEYS）：按模式筛选，全量计数 + 截断返回。
pub async fn keys(
    State(state): State<Arc<AppState>>,
    Query(q): Query<KeysQuery>,
) -> Json<serde_json::Value> {
    let client = match redis_client(&state) {
        Ok(c) => c,
        Err(e) => return Json(json!({ "ok": false, "error": e })),
    };
    let cred = redis_credential::load(&state_dir(&state));
    let pattern = q.pattern.trim().to_string();
    let pattern = if pattern.is_empty() { "*".to_string() } else { pattern };
    let res = blocking(move || -> Result<serde_json::Value, String> {
        let out = run_cli(&client, &cred, &["--scan", "--pattern", &pattern])?;
        let all: Vec<String> = out
            .lines()
            .map(|l| l.trim_end_matches('\r').to_string())
            .filter(|l| !l.is_empty())
            .collect();
        let total = all.len();
        let truncated = total > MAX_KEYS_RETURNED;
        Ok(json!({
            "keys": &all[..total.min(MAX_KEYS_RETURNED)],
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

/// Key 详情：TYPE/TTL/MEMORY USAGE + 按类型取内容。
/// rows 统一为 [标签, 值] 二元组：string → (值, 内容)，hash → (field, value)，
/// list → (序号, 元素)，set → (成员, 成员)，zset → (member, score)。
pub async fn key_detail(
    State(state): State<Arc<AppState>>,
    Query(q): Query<KeyQuery>,
) -> Json<serde_json::Value> {
    let client = match redis_client(&state) {
        Ok(c) => c,
        Err(e) => return Json(json!({ "ok": false, "error": e })),
    };
    let cred = redis_credential::load(&state_dir(&state));
    let name = q.name.clone();
    let res = blocking(move || -> Result<serde_json::Value, String> {
        let kind = run_cli(&client, &cred, &["TYPE", &name])?.trim().to_string();
        if kind == "none" {
            return Err("Key 不存在（可能已被删除或过期）".to_string());
        }
        let ttl_raw = run_cli(&client, &cred, &["TTL", &name])?.trim().to_string();
        let ttl: i64 = ttl_raw.parse().unwrap_or(-3);
        let memory_raw = run_cli(&client, &cred, &["MEMORY", "USAGE", &name])?
            .trim()
            .to_string();
        let memory: Option<u64> = memory_raw.parse().ok();

        let (total, rows): (usize, Vec<Vec<String>>) = match kind.as_str() {
            "string" => {
                let v = value_of(&run_cli(&client, &cred, &["GET", &name])?);
                (1, vec![vec!["值".into(), v]])
            }
            "hash" => {
                let out = run_cli(&client, &cred, &["HGETALL", &name])?;
                let lines: Vec<&str> = out.lines().filter(|l| !l.trim_end_matches('\r').is_empty()).collect();
                let rows: Vec<Vec<String>> = lines
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| vec![c[0].trim_end_matches('\r').to_string(), value_of(c[1])])
                    .collect();
                (rows.len(), rows)
            }
            "list" => {
                let len: usize = run_cli(&client, &cred, &["LLEN", &name])?
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let take = len.min(200);
                let out = run_cli(&client, &cred, &["LRANGE", &name, "0", &(take.saturating_sub(1)).to_string()])?;
                let rows: Vec<Vec<String>> = out
                    .lines()
                    .filter(|l| !l.trim_end_matches('\r').is_empty() || true)
                    .enumerate()
                    .take(take)
                    .map(|(i, l)| vec![format!("[{}]", i), value_of(l)])
                    .collect();
                (len, rows)
            }
            "set" => {
                let len: usize = run_cli(&client, &cred, &["SCARD", &name])?
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let out = run_cli(&client, &cred, &["SMEMBERS", &name])?;
                let rows: Vec<Vec<String>> = out
                    .lines()
                    .take(len.min(200))
                    .map(|l| vec!["成员".into(), value_of(l)])
                    .collect();
                (len, rows)
            }
            "zset" => {
                let len: usize = run_cli(&client, &cred, &["ZCARD", &name])?
                    .trim()
                    .parse()
                    .unwrap_or(0);
                let out = run_cli(&client, &cred, &["ZRANGE", &name, "0", "199", "WITHSCORES"])?;
                let lines: Vec<&str> = out.lines().filter(|l| !l.trim_end_matches('\r').is_empty()).collect();
                let rows: Vec<Vec<String>> = lines
                    .chunks(2)
                    .filter(|c| c.len() == 2)
                    .map(|c| vec![value_of(c[0]), c[1].trim_end_matches('\r').to_string()])
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
    let client = match redis_client(&state) {
        Ok(c) => c,
        Err(e) => return Json(json!({ "ok": false, "error": e })),
    };
    let cred = redis_credential::load(&state_dir(&state));
    let name = req.name;
    let res = blocking(move || -> Result<u64, String> {
        let out = run_cli(&client, &cred, &["DEL", &name])?;
        let n: u64 = out.trim().parse().unwrap_or(0);
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
    let client = match redis_client(&state) {
        Ok(c) => c,
        Err(e) => return Json(json!({ "ok": false, "error": e })),
    };
    let cred = redis_credential::load(&state_dir(&state));
    let name = req.name;
    let seconds = req.seconds;
    let res = blocking(move || -> Result<String, String> {
        if seconds == -1 {
            let out = run_cli(&client, &cred, &["PERSIST", &name])?;
            let n: u64 = out.trim().parse().unwrap_or(0);
            if n == 0 {
                Err("PERSIST 未生效（Key 不存在或本就无过期时间）".to_string())
            } else {
                Ok("已取消过期时间（永久保留）".to_string())
            }
        } else if seconds > 0 {
            let s = seconds.to_string();
            let out = run_cli(&client, &cred, &["EXPIRE", &name, &s])?;
            let n: u64 = out.trim().parse().unwrap_or(0);
            if n == 0 {
                Err("EXPIRE 未生效（Key 不存在）".to_string())
            } else {
                Ok(format!("已设置 {seconds} 秒后过期"))
            }
        } else {
            Err("秒数必须为正整数，或 -1 表示永久".to_string())
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
    fn run_cli_reports_error_lines_without_exit_code() {
        // run_cli 的错误识别逻辑独立验证：redis-cli 命令失败时 exit 0 + (error) 输出
        let text = "(error) WRONGPASS invalid username-password pair\n";
        let hit = text
            .lines()
            .filter(|l| l.contains("(error)") || l.contains("Could not connect") || l.contains("WRONGPASS"))
            .last()
            .map(|l| l.trim().to_string());
        assert_eq!(hit.as_deref(), Some("(error) WRONGPASS invalid username-password pair"));
    }

    #[test]
    fn value_of_truncates_long_values() {
        let long = "x".repeat(70_000);
        let v = value_of(&long);
        assert!(v.contains("已截断"));
        assert!(v.chars().count() < MAX_VALUE_CHARS + 60);
    }
}
