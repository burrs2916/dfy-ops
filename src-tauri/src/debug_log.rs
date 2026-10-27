//! dfy-ops 会话级诊断日志。
//!
//! 独立于 tracing 管道（tracing 打 stdout 用于开发期），debug.log 是 plain-text、
//! 启动时截断的诊断文件——用户提交 bug 时要附这个文件。结构参考
//! biospehere-terminal-app 的 infra/logging/debug_log.rs，简化到够用即可。
//!
//! 格式：
//!     [1735345678] #000001 12:34:56.789 MQTT  phase=connect  evt=tcp.start
//!         | host=192.168.56.1 port=18830 timeout=10s
//!
//! 单条记录独占一行（换行/制表符转义为 \\n / \\t），方便 tail -f / grep。

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

pub const DEBUG_LOG_MAX_SIZE: u64 = 5 * 1024 * 1024; // 5 MB 截断
const MAX_DETAIL_LEN: usize = 4096; // 单条详情上限，防止一条噪音淹没整个会话

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);

/// 进程启动时调用一次，注册 debug.log 写入路径。后续调用忽略。
pub fn init(path: PathBuf) {
    let _ = LOG_PATH.set(path);
}

/// 把 debug.log 解析为 exe 同级目录下的 `debug.log`。
/// Windows 上 exe 在安装目录旁边，macOS 上 `tauri dev` 也会落在项目 target/ 旁边。
/// 如果 `std::env::current_exe()` 解析失败，退而写入当前工作目录。
pub fn resolve_default_path() -> PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    exe_dir.join("debug.log")
}

/// 追加一行到 debug.log。Best-effort：错了静默丢弃——debug.log 挂了不能拖垮主流程。
/// 文件超过 5MB 时先截断再写，防止单会话内无限增长。
fn write_line(message: &str) {
    let Some(path) = LOG_PATH.get() else {
        #[cfg(debug_assertions)]
        eprintln!("[debug_log:uninit] {message}");
        return;
    };

    // 确保目录存在
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // 超 5MB 先截断
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() >= DEBUG_LOG_MAX_SIZE {
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(path);
        }
    }

    let mut file = match std::fs::OpenOptions::new().create(true).append(true).open(path) {
        Ok(f) => f,
        Err(_) => return,
    };

    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let time_ms = chrono::Local::now().format("%H:%M:%S%.3f");
    let _ = writeln!(file, "[{unix}] #{seq:06} {time_ms} {message}");
}

/// 暴露给 lib.rs：打印初始化信息 + 完整路径（方便在 stdout 看到写哪了）。
pub fn on_app_start() {
    let Some(path) = LOG_PATH.get() else { return };
    write_line("===== DFY Ops debug.log 启动 =====");
    write_line(&format!("debug.log 路径：{}", path.display()));
}

// ── 便捷 API ────────────────────────────────────────────────────
// 按「scope + phase + evt」组织日志条，grep 起来方便：
//   MQTT phase=connect evt=tcp.start
//   MQTT phase=connect evt=tcp.ok         detail="tcp 握手 2ms"
//   MQTT phase=connect evt=client.new      detail="rumqttc Client::new() 已创建"
//   MQTT phase=connect evt=recv.start      detail="阻塞等 CONNACK，timeout=10s"
//   MQTT phase=connect evt=recv.result     detail="Ok(Incoming(ConnAck(Success)))"
//   MQTT phase=connect evt=connack.code    detail="rc=0 (Success)"

/// 结构化 entry：scope / phase / evt 都很短，详情可长（会被 clamp + 换行转义）。
pub fn entry(scope: &str, level: &str, phase: &str, evt: &str, detail: &str) {
    let msg = format!(
        "{level:<5} {scope:<6} phase={phase} evt={evt} | {}",
        clamp(&one_line(detail), MAX_DETAIL_LEN)
    );
    write_line(&msg);
}

// ── 工具函数 ────────────────────────────────────────────────────

fn one_line(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

fn clamp(value: &str, max: usize) -> String {
    let total = value.chars().count();
    if total <= max {
        return value.to_string();
    }
    let head: String = value.chars().take(max).collect();
    format!("{head}…<truncated, total {total} chars>")
}

// ── 便捷 scope 常量 ─────────────────────────────────────────────

pub const MQTT_SCOPE: &str = "MQTT";
#[allow(dead_code)]
pub const REDIS_SCOPE: &str = "Redis";
#[allow(dead_code)]
pub const HTTP_SCOPE: &str = "HTTP";
#[allow(dead_code)]
pub const APP_SCOPE: &str = "App";

// ── 秘密值脱敏 ───────────────────────────────────────────────────

#[allow(dead_code)]
pub fn redact(value: &str, secrets: &[&str]) -> String {
    let mut out = value.to_string();
    for secret in secrets {
        if secret.len() < 3 { continue; }
        if out.contains(secret) {
            out = out.replace(secret, "***REDACTED***");
        }
    }
    out
}

pub fn secret_shape(s: Option<&str>) -> String {
    match s {
        Some(v) if !v.is_empty() => format!("set(len={})", v.chars().count()),
        Some(_) => "empty".into(),
        None => "none".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_escapes_newlines() {
        assert_eq!(one_line("a\nb\tc"), "a\\nb\\tc");
    }

    #[test]
    fn clamp_keeps_multibyte_boundaries() {
        let s = "中文输出";
        assert!(clamp(s, 2).starts_with("中文"));
    }

    #[test]
    fn clamp_marks_truncation() {
        let s = "x".repeat(MAX_DETAIL_LEN + 5);
        let out = clamp(&s, MAX_DETAIL_LEN);
        assert!(out.contains("truncated"));
        assert!(out.contains(&(MAX_DETAIL_LEN + 5).to_string()));
    }

    #[test]
    fn redact_masks_password() {
        assert!(redact("password=secret123", &["secret123"]).contains("***REDACTED***"));
        assert!(!redact("password=secret123", &["secret123"]).contains("secret123"));
    }

    #[test]
    fn redact_ignores_short_secrets() {
        // 2 字符的 mask 太泛，跳过
        assert_eq!(redact("ls -la", &["la"]), "ls -la");
    }

    #[test]
    fn secret_shape_never_leaks_value() {
        assert_eq!(secret_shape(Some("hunter2")), "set(len=7)");
        assert_eq!(secret_shape(Some("")), "empty");
        assert_eq!(secret_shape(None), "none");
    }
}
