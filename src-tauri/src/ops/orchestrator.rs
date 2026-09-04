use std::collections::{HashMap, HashSet, VecDeque};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::ops::health;
use crate::ops::process::ProcManager;
use crate::ops::registry::ServiceDef;

/// 单次动作的结果。
#[derive(Debug, Clone, Serialize)]
pub struct ActionResult {
    pub id: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// 启动后等待服务就绪的超时（秒）。
const READY_TIMEOUT_SECS: u64 = 30;
/// 就绪轮询间隔（毫秒）。
const READY_POLL_MS: u64 = 500;

/// 等待服务端口全部可连（无端口的桌面客户端仅短暂等待）。
fn wait_ready(def: &ServiceDef, timeout_secs: u64) -> bool {
    if def.ports.is_empty() {
        thread::sleep(Duration::from_millis(500));
        return true;
    }
    let start = Instant::now();
    loop {
        if def
            .ports
            .iter()
            .all(|p| health::check_tcp("127.0.0.1", *p, 800))
        {
            return true;
        }
        if start.elapsed().as_secs() >= timeout_secs {
            return false;
        }
        thread::sleep(Duration::from_millis(READY_POLL_MS));
    }
}

/// 按依赖拓扑排序返回启动顺序（被依赖者在前）。检测到环则返回 Err。
pub fn start_order(defs: &[ServiceDef]) -> Result<Vec<String>, String> {
    let mut indeg: HashMap<&str, usize> = HashMap::new();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for d in defs {
        indeg.entry(d.id.as_str()).or_insert(0);
        for dep in &d.depends_on {
            adj.entry(dep.as_str()).or_default().push(d.id.as_str());
            *indeg.entry(d.id.as_str()).or_insert(0) += 1;
        }
    }
    let mut q: VecDeque<&str> = indeg
        .iter()
        .filter(|(_, &v)| v == 0)
        .map(|(&k, _)| k)
        .collect();
    let mut order = Vec::new();
    while let Some(n) = q.pop_front() {
        order.push(n.to_string());
        if let Some(ns) = adj.get(n) {
            for m in ns {
                let e = indeg.get_mut(m).unwrap();
                *e -= 1;
                if *e == 0 {
                    q.push_back(m);
                }
            }
        }
    }
    if order.len() != defs.len() {
        return Err("服务依赖存在环，无法排序".into());
    }
    Ok(order)
}

/// 停止顺序为启动顺序的逆序（先停依赖方，后停被依赖方）。
pub fn stop_order(defs: &[ServiceDef]) -> Result<Vec<String>, String> {
    Ok(start_order(defs)?.into_iter().rev().collect())
}

fn act_all(
    procs: &ProcManager,
    defs: &[ServiceDef],
    order: Vec<String>,
    start: bool,
) -> Vec<ActionResult> {
    order
        .into_iter()
        .filter_map(|id| {
            let def = defs.iter().find(|d| d.id == id)?;
            // 停止阶段：跳过当前未运行的服务，避免无意义报错
            if !start && !procs.is_running(&id) {
                return None;
            }
            let r = if start {
                procs.start(&id, &def.install_dir, &def.run_cmd)
            } else {
                procs.stop(&id)
            };
            Some(ActionResult {
                id,
                ok: r.is_ok(),
                error: r.err().map(|e| e.to_string()),
            })
        })
        .collect()
}

/// 按依赖顺序启动全部常驻服务；每启动一个，等待其端口就绪后再启动依赖方。
pub fn start_all(procs: &ProcManager, defs: &[ServiceDef]) -> Vec<ActionResult> {
    match start_order(defs) {
        Ok(order) => order
            .into_iter()
            .filter_map(|id| {
                let def = defs.iter().find(|d| d.id == id)?;
                if !procs.is_running(&id) {
                    if let Err(e) = procs.start(&id, &def.install_dir, &def.run_cmd) {
                        return Some(ActionResult {
                            id,
                            ok: false,
                            error: Some(e.to_string()),
                        });
                    }
                }
                let ready = wait_ready(def, READY_TIMEOUT_SECS);
                Some(ActionResult {
                    id,
                    ok: ready,
                    error: if ready {
                        None
                    } else {
                        Some("启动后未在超时内就绪".into())
                    },
                })
            })
            .collect(),
        Err(e) => vec![ActionResult {
            id: "*".into(),
            ok: false,
            error: Some(e),
        }],
    }
}

/// 按逆依赖顺序停止全部服务。
pub fn stop_all(procs: &ProcManager, defs: &[ServiceDef]) -> Vec<ActionResult> {
    match stop_order(defs) {
        Ok(order) => act_all(procs, defs, order, false),
        Err(e) => vec![ActionResult {
            id: "*".into(),
            ok: false,
            error: Some(e),
        }],
    }
}

/// 启动单个服务，并先确保其全部（传递）依赖已启动且就绪。
pub fn start_with_deps(
    procs: &ProcManager,
    defs: &[ServiceDef],
    id: &str,
) -> Vec<ActionResult> {
    // 收集 id 及其所有传递依赖
    let mut need: Vec<String> = Vec::new();
    let mut stack = vec![id.to_string()];
    let mut seen = HashSet::new();
    while let Some(cur) = stack.pop() {
        if !seen.insert(cur.clone()) {
            continue;
        }
        if let Some(d) = defs.iter().find(|d| d.id == cur) {
            for dep in &d.depends_on {
                stack.push(dep.clone());
            }
            need.push(cur);
        }
    }
    let sub: Vec<ServiceDef> = need
        .iter()
        .filter_map(|n| defs.iter().find(|d| d.id == *n).cloned())
        .collect();
    match start_order(&sub) {
        Ok(order) => order
            .into_iter()
            .filter_map(|sid| {
                let def = defs.iter().find(|d| d.id == sid)?;
                if !procs.is_running(&sid) {
                    if let Err(e) = procs.start(&sid, &def.install_dir, &def.run_cmd) {
                        return Some(ActionResult {
                            id: sid,
                            ok: false,
                            error: Some(e.to_string()),
                        });
                    }
                }
                let ready = wait_ready(def, READY_TIMEOUT_SECS);
                Some(ActionResult {
                    id: sid,
                    ok: ready,
                    error: if ready {
                        None
                    } else {
                        Some("启动后未在超时内就绪".into())
                    },
                })
            })
            .collect(),
        Err(e) => vec![ActionResult {
            id: id.into(),
            ok: false,
            error: Some(e),
        }],
    }
}
