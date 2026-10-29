//! 常驻服务保活看门狗（P2-f）。
//!
//! 职责：对**平台拉起过**的常驻服务（is_daemon）做周期探活，发现进程退出即自动
//! 重新拉起——补上「is_daemon 只有标记没有实现」的缺口，是 P1-4 真实探活的
//! 自然延伸：探活让崩溃及时可见，看门狗让崩溃及时自愈。
//!
//! 边界（刻意收窄）：
//! - 只接管 `ever_started`（平台至少成功 start 过一次）的服务。平台从未碰过的
//!   外部服务（detected）不归看门狗管——「进程是事实来源」，平台不替用户决定
//!   哪些服务必须活着。
//! - 用户显式 stop 的服务进入抑制名单，不再自动拉起（尊重「我就是想让它停着」），
//!   下一次显式 start 解除抑制。
//! - 同一服务两次拉起之间有最小间隔（冷却期）：若拉起后进程立刻又死（配置错误、
//!   端口冲突等），避免变成高频 crash-loop。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::ops::process::ProcManager;
use crate::ops::registry::ServiceDef;

/// debug.log 里看门狗自动拉起的结构化 scope（与 Proc/App 并列，可按 scope 过滤）。
/// 「没人动它怎么自己起来了」的困惑直接在这里给出答案：进程退出 → 看门狗拉起。
const WATCHDOG_SCOPE: &str = "Watchdog";

/// 同一服务两次自动拉起的最小间隔（crash-loop 冷却）。
const RESTART_COOLDOWN: Duration = Duration::from_secs(30);

/// 启动保活看门狗线程（独立 std 线程，不占 tokio worker；内部无阻塞探测，
/// is_running 是 try_wait 的 O(1) 调用）。
pub fn spawn(procs: Arc<ProcManager>, defs: Vec<ServiceDef>, interval_secs: u64) {
    let interval = Duration::from_secs(interval_secs.max(1));
    std::thread::Builder::new()
        .name("keepalive-watchdog".into())
        .spawn(move || run_loop(procs, defs, interval))
        .expect("保活看门狗线程启动失败");
}

fn run_loop(procs: Arc<ProcManager>, defs: Vec<ServiceDef>, interval: Duration) {
    let mut last_attempt: HashMap<String, Instant> = HashMap::new();
    loop {
        std::thread::sleep(interval);
        for id in procs.keepalive_candidates() {
            let Some(def) = defs.iter().find(|d| d.id == id) else {
                continue;
            };
            // 非常驻服务（如 Tauri 桌面客户端）退出是正常生命周期，不保活
            if !def.is_daemon {
                continue;
            }
            if in_cooldown(last_attempt.get(&id).copied(), Instant::now()) {
                tracing::debug!("服务 {id} 在保活冷却期内，暂不重启");
                continue;
            }
            tracing::warn!("受管常驻服务 {id} 进程已退出，自动拉起（保活）");
            last_attempt.insert(id.clone(), Instant::now());
            // 拉起结果落 debug.log（scope=Watchdog）+ ops-events.jsonl（actor=watchdog）：
            // start() 内部已有 Proc scope 日志，这里补「为什么会被拉起」的上下文与审计
            match procs.start(&id, &def.install_dir, &def.run_cmd) {
                Ok(()) => {
                    crate::debug_log::entry(
                        WATCHDOG_SCOPE,
                        "WARN",
                        "keepalive",
                        "restart",
                        &format!("服务 {id} 进程已退出，已自动拉起（run_cmd={}）", def.run_cmd),
                    );
                    crate::ops_events::record("watchdog", &id, "autostart", true, "进程退出，看门狗自动拉起");
                }
                Err(e) => {
                    crate::debug_log::entry(
                        WATCHDOG_SCOPE,
                        "ERROR",
                        "keepalive",
                        "restart_failed",
                        &format!("服务 {id} 自动拉起失败：{e}"),
                    );
                    crate::ops_events::record("watchdog", &id, "autostart", false, &format!("自动拉起失败：{e}"));
                }
            }
        }
    }
}

/// 是否处于 crash-loop 冷却期：从未尝试过 → false；距上次尝试不足冷却时间 → true。
fn in_cooldown(last: Option<Instant>, now: Instant) -> bool {
    last.map(|t| now.duration_since(t) < RESTART_COOLDOWN)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 冷却判定：从未尝试 → 允许重启；刚试过 → 拒绝；超过冷却期 → 允许。
    #[test]
    fn cooldown_gate() {
        let now = Instant::now();
        assert!(!in_cooldown(None, now));
        assert!(in_cooldown(Some(now), now));
        let long_ago = now - RESTART_COOLDOWN - Duration::from_secs(1);
        assert!(!in_cooldown(Some(long_ago), now));
    }
}
