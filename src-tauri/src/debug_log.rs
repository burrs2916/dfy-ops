//! dfy-ops 会话级诊断日志。
//!
//! 独立于 tracing 管道（tracing 打 stdout 用于开发期），debug.log 是 plain-text、
//! 启动时截断的诊断文件——用户提交 bug 时要附这个文件。结构参考
//! biospehere-terminal-app 的 infra/logging/debug_log.rs，简化到够用即可。
//!
//! 多路径写入（参考 biosphere 的 resolve_debug_log_paths）：
//!     1. Tauri app_data_dir（Windows = C:\Users\<user>\AppData\Local\<identifier>\debug.log，
//!        macOS = ~/Library/Application Support/<identifier>/debug.log）——**永远有写权限**
//!     2. exe 同级目录（debug 构建用户最容易找到；release 装 Program Files 可能无权限）
//!     3. 项目 target/debug/debug.log（dev 构建用户最直接能看到的）
//! 三处都写，用户随便找一个就行。
//!
//! 格式：
//!     [1735345678] #000001 12:34:56.789 INFO  MQTT  phase=connect evt=enter
//!         | host=192.168.56.1 port=18830 user=guest pwd=set(len=6)
//!
//! 单条记录独占一行（换行/制表符转义为 \\n / \\t），方便 tail -f / grep。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

pub const DEBUG_LOG_MAX_SIZE: u64 = 5 * 1024 * 1024; // 5 MB 截断
const MAX_DETAIL_LEN: usize = 4096; // 单条详情上限，防止一条噪音淹没整个会话

type DebugLogPaths = Arc<Vec<PathBuf>>;

static PATHS: OnceLock<DebugLogPaths> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);

/// 进程启动时调用一次，注册 debug.log 写入路径。后续调用忽略。
/// 只接一个路径：**exe 同级目录下的 debug.log**。
pub fn init(path: PathBuf) {
    let _ = PATHS.set(Arc::new(vec![path]));
}

/// 追加一行到**所有**已注册路径下的 debug.log。
/// Best-effort：错了静默丢弃——debug.log 挂了不能拖垮主流程。
/// 文件超过 5MB 时先截断再写。
fn write_line_all(message: &str) {
    let Some(paths) = PATHS.get() else {
        #[cfg(debug_assertions)]
        eprintln!("[debug_log:uninit] {message}");
        return;
    };
    for path in paths.iter() {
        let _ = write_line(path, message);
    }
}

/// 追加一行到单个 debug.log。内部：创建目录、超 5MB 截断、追加写入。
fn write_line(path: &Path, message: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // 超 5MB 先截断
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() >= DEBUG_LOG_MAX_SIZE {
            let _ = std::fs::OpenOptions::new().write(true).truncate(true).open(path);
        }
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let time_ms = chrono::Local::now().format("%H:%M:%S%.3f");
    writeln!(file, "[{unix}] #{seq:06} {time_ms} {message}")?;
    Ok(())
}

/// 启动标记：打一行启动标记。只有一个路径（exe 同级 debug.log）。
pub fn on_app_start() {
    write_line_all("===== DFY Ops debug.log 启动 =====");
}

// ── 便捷 API ────────────────────────────────────────────────────

/// 结构化 entry：scope / level / phase / evt 都很短，详情可长（会被 clamp + 换行转义）。
pub fn entry(scope: &str, level: &str, phase: &str, evt: &str, detail: &str) {
    let msg = format!(
        "{level:<5} {scope:<6} phase={phase} evt={evt} | {}",
        clamp(&one_line(detail), MAX_DETAIL_LEN)
    );
    write_line_all(&msg);
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
        assert_eq!(redact("ls -la", &["la"]), "ls -la");
    }

    #[test]
    fn secret_shape_never_leaks_value() {
        assert_eq!(secret_shape(Some("hunter2")), "set(len=7)");
        assert_eq!(secret_shape(Some("")), "empty");
        assert_eq!(secret_shape(None), "none");
    }
}
