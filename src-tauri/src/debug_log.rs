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
use std::sync::{Arc, OnceLock};

pub const DEBUG_LOG_MAX_SIZE: u64 = 5 * 1024 * 1024; // 5 MB 截断
const MAX_DETAIL_LEN: usize = 4096;

pub type DebugLogPaths = Arc<Vec<PathBuf>>;

static PATHS: OnceLock<DebugLogPaths> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);

/// 注册所有 debug.log 写入路径，进程启动时调用一次。后续调用忽略。
pub fn init_paths(paths: DebugLogPaths) {
    let _ = PATHS.set(paths);
}

/// 返回已注册路径的副本；init_paths 之前调用返回空。
/// 供 /api/diagnostics 上报给前端——应用自己报告日志写在哪，用户不用猜。
pub fn registered_paths() -> Vec<PathBuf> {
    PATHS.get().map(|p| p.as_ref().clone()).unwrap_or_default()
}

/// 读取 debug.log 尾部 n 行（取第一个读得到的已注册路径）。
/// 供诊断快照回显——用户把快照贴给维护方时，最近的日志现场一起带上，
/// 省一次打开文件的来回。未初始化/读不到时返回空（快照照常生成）。
pub fn tail_lines(n: usize) -> Vec<String> {
    let Some(paths) = PATHS.get() else {
        return Vec::new();
    };
    for path in paths.iter() {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        let lines: Vec<&str> = content.lines().collect();
        let start = lines.len().saturating_sub(n);
        return lines[start..]
            .iter()
            .map(|l| {
                // entry 已限 4096 字符，但 write_line 直写不限——防个别长行撑爆快照
                if l.chars().count() > 500 {
                    let head: String = l.chars().take(500).collect();
                    format!("{head}…")
                } else {
                    (*l).to_string()
                }
            })
            .collect();
    }
    Vec::new()
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
    // 整行先拼内存、再一次 write_all 落盘：`writeln!` 会按格式分段发起多个
    // write syscall，API 线程与看门狗/启停操作并发写时行会互相穿插撕裂，
    // 排障时看到半截日志等于没看到。O_APPEND + 单次写 = 行级原子。
    let line = format!("[{unix}] #{seq:06} {time_ms} {message}\n");
    file.write_all(line.as_bytes())?;
    Ok(())
}

/// 追加到**所有**已注册路径。内部逐条写，错了不影响其他路径。
pub fn write_to(paths: &DebugLogPaths, message: &str) {
    for path in paths.iter() {
        if let Err(e) = write_line(path, message) {
            // Windows GUI 子系统下 stderr 无效，eprintln 会 panic——只在 debug 构建打；
            // release 构建不用 e，加 _ 前缀消 unused 警告（CI 声音要干净）
            #[cfg(debug_assertions)]
            eprintln!("[debug_log] 写入失败 {}: {e}", path.display());
            #[cfg(not(debug_assertions))]
            let _ = e;
        }
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

// ── tracing → debug.log 桥 ────────────────────────────────────────
//
// 背景：tracing 此前只打 stdout。Windows 发布态是 GUI 子系统（没有控制台），
// 端口绑定失败、运维 API 异常退出这类启动期关键错误一个字都留不下——
// 用户拿 debug.log 排障时完全看不到，正是「出错时日志里定位不到」的元凶之一。
//
// 本 writer 让所有 tracing 事件双写：stdout（开发态终端，保留颜色）+
// debug.log（发布态唯一可达落点，剥掉 ANSI 后按 debug.log 前缀落盘）。
// 用法（lib.rs）：tracing_subscriber::fmt().with_writer(TraceMakeWriter)

pub struct TraceMakeWriter;

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for TraceMakeWriter {
    type Writer = TraceWriter;
    fn make_writer(&'a self) -> TraceWriter {
        TraceWriter { buf: String::new() }
    }
}

pub struct TraceWriter {
    buf: String,
}

impl std::io::Write for TraceWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // stdout 镜像：Windows GUI 子系统下句柄无效——错误忽略，绝不能 panic
        let _ = std::io::stdout().write_all(buf);
        self.buf.push_str(&String::from_utf8_lossy(buf));
        // fmt 先写事件内容、再单独补一个换行——攒到完整行才落盘
        while let Some(pos) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=pos).collect();
            emit_trace(&line);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            emit_trace(&line);
        }
        Ok(())
    }
}

impl Drop for TraceWriter {
    fn drop(&mut self) {
        let _ = std::io::Write::flush(self);
    }
}

/// 一行 tracing 输出落盘：剥 ANSI、去尾部换行，空行不写。
fn emit_trace(line: &str) {
    let clean = strip_ansi(line);
    let clean = clean.trim_end();
    if !clean.is_empty() {
        write(clean);
    }
}

/// 去 ANSI 转义序列：tracing 的彩色输出（ESC [ ... m 等 CSI 序列）落盘前清洗，
/// debug.log 保持纯文本——带转义码的行既难读也干扰检索。
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if it.peek() == Some(&'[') {
            // CSI 序列：ESC [ ... 终止于 0x40..=0x7E（tracing 的颜色码都属此类）
            it.next();
            for c2 in it.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&c2) {
                    break;
                }
            }
        } else {
            // 其他孤立 ESC 序列：连同后一个字符一起丢弃
            let _ = it.next();
        }
    }
    out
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
    fn strip_ansi_removes_color_codes() {
        // tracing 彩色输出的典型 CSI 颜色码 + 普通文本
        assert_eq!(strip_ansi("\u{1b}[32mINFO\u{1b}[0m hello"), "INFO hello");
        // 多段颜色码混合
        assert_eq!(strip_ansi("\u{1b}[1m\u{1b}[31merr\u{1b}[0m"), "err");
        // 无转义的纯文本原样保留
        assert_eq!(strip_ansi("plain 中文"), "plain 中文");
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
