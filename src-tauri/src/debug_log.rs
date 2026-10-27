//! dfy-ops 会话级诊断日志。
//!
//! 独立于 tracing 管道（tracing 打 stdout 用于开发期），debug.log 是 plain-text、
//! 启动时截断的诊断文件——用户提交 bug 时要附这个文件。
//!
//! 参考 biosphere-terminal-app 的 infra/logging/debug_log.rs。
//!
//! 多路径写入（和 biosphere 完全一致的策略）：
//!     1. dirs_next::data_local_dir  （Windows = %LOCALAPPDATA%\identifier\debug.log，
//!        macOS = ~/Library/Application Support/identifier/debug.log）——永远有写权限
//!     2. APPDATA 兜底（Windows）
//!     3. exe 同级目录——用户最直觉找的位置，但 Program Files 下可能无权限
//! 三处都写，总有一处成功。用户看 stdout 里 tracing 输出的路径也行，
//! 找 exe 旁边也行。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub const DEBUG_LOG_MAX_SIZE: u64 = 5 * 1024 * 1024; // 5 MB 截断
const MAX_DETAIL_LEN: usize = 4096;

pub type DebugLogPaths = Arc<Vec<PathBuf>>;

static PATHS: OnceLock<DebugLogPaths> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);
/// 每个路径最近一次写入失败的错误（路径 -> 错误文本）。
/// 供 /api/diagnostics 上报——写不进去了要让用户在界面上直接看到原因。
static LAST_ERRORS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// 注册所有 debug.log 写入路径，进程启动时调用一次。后续调用忽略。
pub fn init_paths(paths: DebugLogPaths) {
    let _ = PATHS.set(paths);
}

/// 返回已注册路径的副本；init_paths 之前调用返回空。
/// 供 /api/diagnostics 上报给前端——应用自己报告日志写在哪，用户不用猜。
pub fn registered_paths() -> Vec<PathBuf> {
    PATHS.get().map(|p| p.as_ref().clone()).unwrap_or_default()
}

/// 追加一行到单个 debug.log 文件。
/// Best-effort：返回 Result 但调用方可以 `.ok()` 忽略——debug.log 挂了不能拖垮主流程。
/// 文件超过 5MB 时先截断再写。
pub fn write_line(path: &Path, message: &str) -> std::io::Result<()> {
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

/// 追加到**所有**已注册路径。内部逐条写，错误记入 LAST_ERRORS（界面上报）。
pub fn write_to(paths: &DebugLogPaths, message: &str) {
    for path in paths.iter() {
        if let Err(e) = write_line(path, message) {
            let key = path.display().to_string();
            let mut errs = LAST_ERRORS.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(slot) = errs.iter_mut().find(|(k, _)| *k == key) {
                slot.1 = e.to_string();
            } else {
                errs.push((key, e.to_string()));
            }
            // Windows GUI 子系统下 stderr 无效，eprintln 会 panic——只在 debug 构建打
            #[cfg(debug_assertions)]
            eprintln!("[debug_log] 写入失败 {}: {e}", path.display());
        }
    }
}

/// 各路径最近一次写入错误（路径, 错误）。
pub fn last_errors() -> Vec<(String, String)> {
    LAST_ERRORS.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// 读取第一个已存在的日志文件内容（最多 max 字节，从头截取）。
/// 供「关于」页直接展示日志内容——用户连文件夹都不用打开。
pub fn read_log_head(max: usize) -> Option<String> {
    let paths = PATHS.get()?;
    let path = paths.iter().find(|p| p.exists())?;
    match std::fs::read(path) {
        Ok(bytes) => {
            let slice = &bytes[..bytes.len().min(max)];
            Some(String::from_utf8_lossy(slice).to_string())
        }
        Err(e) => Some(format!("<读取失败：{e}>")),
    }
}

/// 追加到进程全局 debug.log 位置。init_paths 之前调用会静默丢弃。
pub fn write(message: &str) {
    match PATHS.get() {
        Some(paths) => write_to(paths, message),
        None => {
            #[cfg(debug_assertions)]
            eprintln!("[debug_log:uninit] {message}");
        }
    }
}

// ── 结构化 entry（仿 biosphere 的 structured，但简化成我们需要的 scope）───

pub const MQTT_SCOPE: &str = "MQTT";
#[allow(dead_code)]
pub const REDIS_SCOPE: &str = "Redis";
#[allow(dead_code)]
pub const HTTP_SCOPE: &str = "HTTP";
#[allow(dead_code)]
pub const APP_SCOPE: &str = "App";

/// 结构化 entry：scope + level + phase + evt + detail
/// 格式：
///     [MQTT] #000042 12:34:56.789 INFO  phase=connect evt=enter | host=192.168.56.1 port=18830 ...
pub fn entry(scope: &str, level: &str, phase: &str, evt: &str, detail: &str) {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let time_ms = chrono::Local::now().format("%H:%M:%S%.3f");
    let line = format!(
        "[{scope}] #{seq:06} {time_ms} {level:<5} phase={phase} evt={evt} | {}",
        clamp(&one_line(detail), MAX_DETAIL_LEN)
    );
    write(&line);
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
