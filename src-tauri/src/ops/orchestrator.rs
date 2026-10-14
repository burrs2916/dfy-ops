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
            // 第三方依赖不归平台托管（由系统服务/容器运行时管理），启停一律跳过
            if def.third_party {
                return None;
            }
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
///
/// 只启动常驻服务（`is_daemon = true`）：桌面客户端这类非常驻程序（`is_daemon = false`）
/// 不参与批量启动，否则「全部启动」会在运维机上直接弹出一个 GUI 窗口。
///
/// 过滤放在拓扑排序**之后**——先对全量 defs 排序得出完整顺序，再按 id 过滤。
/// 若改成「先过滤再排序」，某个常驻服务依赖了一个非常驻服务时，被依赖方不在输入集合里，
/// 其入度永远无法归零，会被 `start_order` 误判成「依赖存在环」而整体报错。
pub fn start_all(procs: &ProcManager, defs: &[ServiceDef]) -> Vec<ActionResult> {
    let daemon_ids: HashSet<&str> = defs
        .iter()
        .filter(|d| d.is_daemon && !d.third_party)
        .map(|d| d.id.as_str())
        .collect();
    match start_order(defs) {
        Ok(order) => order
            .into_iter()
            .filter(|id| daemon_ids.contains(id.as_str()))
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
pub fn start_with_deps(procs: &ProcManager, defs: &[ServiceDef], id: &str) -> Vec<ActionResult> {
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
                // 第三方依赖（如 edge-conductor 依赖的 vernemq/minio）：不托管，
                // 跳过启动但不算失败——其运行由系统服务/容器运行时负责。
                if def.third_party {
                    return None;
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::registry::ServiceKind;

    fn def(id: &str, is_daemon: bool) -> ServiceDef {
        ServiceDef {
            id: id.into(),
            name: id.into(),
            kind: ServiceKind::RustBin,
            java_major: None,
            java_home: None,
            install_dir: ".".into(),
            build_cmd: None,
            // Unix 下 sh -c true 立即退出；Windows 下 cmd /C true 会 spawn 失败。
            // 两种结果本测试都能接受——断言只关心「哪些 id 被纳入启动」，不关心成败。
            run_cmd: "true".into(),
            ports: vec![],
            process_names: vec![],
            product_names: vec![],
            health_url: None,
            log_paths: vec![],
            depends_on: vec![],
            requires: vec![],
            is_daemon,
            third_party: false,
            description: String::new(),
        }
    }

    /// 回归：start_all 不得拉起第三方依赖（vernemq/minio）——平台只检测不托管。
    #[test]
    fn start_all_skips_third_party() {
        let mut mq = def("vernemq", true);
        mq.third_party = true;
        let defs = vec![def("daemon-a", true), mq];
        let procs = ProcManager::new();
        let out = start_all(&procs, &defs);
        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["daemon-a"],
            "第三方依赖不应被「全部启动」拉起，实际：{ids:?}"
        );
        let _ = procs.stop("daemon-a");
    }

    /// 回归：start_all 只能拉起常驻服务。
    /// 桌面客户端（is_daemon = false）被纳入批量启动时，会在运维机上弹出 GUI 窗口。
    #[test]
    fn start_all_skips_non_daemon() {
        let defs = vec![def("daemon-a", true), def("desktop-app", false)];
        let procs = ProcManager::new();
        let out = start_all(&procs, &defs);

        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["daemon-a"], "非常驻服务不应被「全部启动」拉起");

        let _ = procs.stop("daemon-a");
    }

    /// 回归：常驻服务依赖非常驻服务时，不得误报「依赖存在环」。
    /// 这是「先排序再过滤」而非「先过滤再排序」的原因——若先过滤，被依赖方不在输入集合里，
    /// 其入度永远无法归零，`start_order` 会把它判成环并让整个批量启动失败。
    #[test]
    fn start_all_does_not_report_cycle_when_dep_is_non_daemon() {
        let dep = def("desktop-dep", false);
        let mut svc = def("daemon-b", true);
        svc.depends_on = vec!["desktop-dep".into()];

        let defs = [dep, svc];
        let procs = ProcManager::new();
        let out = start_all(&procs, &defs);

        assert!(
            !out.iter()
                .any(|r| r.error.as_deref() == Some("服务依赖存在环，无法排序")),
            "过滤导致的假环必须消除，实际结果：{out:?}"
        );
        assert!(
            out.iter().any(|r| r.id == "daemon-b"),
            "常驻服务自身仍应被尝试启动，实际结果：{out:?}"
        );
        assert!(
            !out.iter().any(|r| r.id == "desktop-dep"),
            "非常驻的依赖服务不应被拉起，实际结果：{out:?}"
        );

        let _ = procs.stop("daemon-b");
    }

    /// 拓扑排序：被依赖者在前；真实存在环时报错。
    #[test]
    fn start_order_respects_dependencies_and_detects_cycle() {
        let mut a = def("a", true);
        let mut b = def("b", true);
        b.depends_on = vec!["a".into()];
        let defs = [a.clone(), b.clone()];
        let order = start_order(&defs).expect("无环时应返回顺序");
        assert_eq!(order, vec!["a", "b"]);
        assert_eq!(stop_order(&defs).unwrap(), vec!["b", "a"]);

        a.depends_on = vec!["b".into()];
        let cyclic = [a, b];
        assert!(start_order(&cyclic).is_err(), "存在环时应返回 Err");
    }
}
