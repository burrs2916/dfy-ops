//! 运维操作事件留痕（启停 / 安装 / 卸载的审计日志）。
//!
//! 与 debug.log 的分工：debug.log 是开发者诊断用（杂、每次启动截断）；
//! ops-events.jsonl 是运维审计用（只记关键操作、跨会话持久、界面可查）。
//! 用户诉求：80 端口常驻服务必须有「启停事件」控制——谁在什么时候
//! 对哪个组件做了什么操作、结果如何，维护时要能回看。
//!
//! 存储位置：与 debug.log 同目录（%LOCALAPPDATA%\identifier\ops-events.jsonl，
//! macOS = ~/Library/Application Support/identifier/）。
//! 格式：每行一条 JSON，超过 512KB 截断（审计不需要无限历史）。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

static EVENTS_PATH: OnceLock<PathBuf> = OnceLock::new();
static SEQ: AtomicU64 = AtomicU64::new(0);
const MAX_FILE_SIZE: u64 = 512 * 1024;
/// 读接口最多返回的事件条数（界面时间轴 + 展开都用它封顶）
pub const RECENT_LIMIT: usize = 100;

/// 注册事件文件路径（进程启动时调用，与 debug_log::init_paths 同源目录）。
pub fn init_path(dir: PathBuf) {
    let _ = EVENTS_PATH.set(dir.join("ops-events.jsonl"));
}

fn path() -> Option<&'static PathBuf> {
    EVENTS_PATH.get()
}

/// 记录一条运维事件。best-effort：写失败不拖垮主流程。
/// - `actor`：操作发起者（user=界面操作 / installer=安装管线 / watchdog=看门狗）
/// - `target`：组件 id（nginx / mysql / dfy-backend…）
/// - `action`：start / stop / install / uninstall / autostart…
/// - `ok`：是否成功
/// - `detail`：一句话结果说明（含失败原因）
pub fn record(actor: &str, target: &str, action: &str, ok: bool, detail: &str) {
    let Some(p) = path() else { return };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(meta) = std::fs::metadata(p) {
        if meta.len() >= MAX_FILE_SIZE {
            let _ = std::fs::OpenOptions::new().write(true).truncate(true).open(p);
        }
    }
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    // id：毫秒时间戳 + 进程内序号，唯一且单调——前端单条删除按它定位
    let id = format!(
        "{}-{:06}",
        chrono::Local::now().timestamp_millis(),
        SEQ.fetch_add(1, Ordering::Relaxed) % 1_000_000
    );
    let line = serde_json::json!({
        "id": id,
        "ts": ts.to_string(),
        "actor": actor,
        "target": target,
        "action": action,
        "ok": ok,
        "detail": detail,
    });
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
        use std::io::Write;
        let _ = writeln!(f, "{line}");
    }
}

/// 事件条目（API 返回给前端的形态）。
#[derive(serde::Serialize, Clone)]
pub struct OpsEvent {
    pub id: String,
    pub ts: String,
    pub actor: String,
    pub target: String,
    pub action: String,
    pub ok: bool,
    pub detail: String,
}

/// 读最近 `limit` 条事件（时间倒序：最新在前）。
pub fn recent(limit: usize) -> Vec<OpsEvent> {
    let Some(p) = path() else { return Vec::new() };
    let Ok(text) = std::fs::read_to_string(p) else { return Vec::new() };
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(limit.max(1));
    lines[start..]
        .iter()
        .filter_map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).ok()?;
            Some(OpsEvent {
                id: v["id"].as_str().unwrap_or("").into(),
                ts: v["ts"].as_str().unwrap_or("").into(),
                actor: v["actor"].as_str().unwrap_or("").into(),
                target: v["target"].as_str().unwrap_or("").into(),
                action: v["action"].as_str().unwrap_or("").into(),
                ok: v["ok"].as_bool().unwrap_or(false),
                detail: v["detail"].as_str().unwrap_or("").into(),
            })
        })
        .rev()
        .collect()
}

/// 清空全部事件（用户在时间轴上点「清空」）。
pub fn clear() {
    let Some(p) = path() else { return };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::File::create(p);
    crate::debug_log::entry(
        crate::ops::process::PROC_SCOPE,
        "INFO",
        "ops_events",
        "clear",
        "用户清空了运维事件时间轴",
    );
}

/// 删除单条事件（按 id 定位；旧格式无 id 的行保留不动）。
pub fn remove(id: &str) -> bool {
    let Some(p) = path() else { return false };
    let Ok(text) = std::fs::read_to_string(p) else { return false };
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| {
            serde_json::from_str::<serde_json::Value>(l)
                .ok()
                .and_then(|v| v["id"].as_str().map(|s| s.to_string()))
                .as_deref()
                != Some(id)
        })
        .collect();
    if kept.len() == text.lines().count() {
        return false; // 没找到目标行
    }
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(p, {
        let mut s = kept.join("\n");
        if !s.is_empty() {
            s.push('\n');
        }
        s
    }) {
        Ok(()) => {
            crate::debug_log::entry(
                crate::ops::process::PROC_SCOPE,
                "INFO",
                "ops_events",
                "remove",
                &format!("删除单条运维事件 id={id}"),
            );
            true
        }
        Err(e) => {
            crate::debug_log::entry(
                crate::ops::process::PROC_SCOPE,
                "ERROR",
                "ops_events",
                "remove_failed",
                &format!("删除事件 id={id} 失败：{e}"),
            );
            false
        }
    }
}
