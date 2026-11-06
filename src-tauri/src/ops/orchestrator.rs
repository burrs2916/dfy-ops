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

/// 启动后等待服务就绪的超时（秒）。30s 对 300MB 的 Spring Boot 冷启动不够
/// （2026-11-02 实测装完即报「未在超时内就绪」但进程其实还在起）——放宽到 90s。
const READY_TIMEOUT_SECS: u64 = 90;
/// 就绪轮询间隔（毫秒）。
const READY_POLL_MS: u64 = 500;
/// 等待进度日志间隔（秒）。
const READY_PROGRESS_SECS: u64 = 10;

/// 等待服务端口全部可连（无端口的桌面客户端仅短暂等待）。
///
/// 超时不再返回笼统的「未就绪」：进程已退出时带回**退出码 + spawn 输出尾部**
/// （秒退的原因就在 platform-spawn.log 里），仍在运行时说明端口未监听——
/// 让「为什么起不来」在日志与任务回显里当场可见。
fn wait_ready(
    procs: &ProcManager,
    id: &str,
    def: &ServiceDef,
    timeout_secs: u64,
) -> Result<(), String> {
    if def.ports.is_empty() {
        thread::sleep(Duration::from_millis(500));
        return Ok(());
    }
    // edge-conductor 的端口可被配置中心修改——就绪等待以 config.toml
    // 实际端口为准（registry 声明值作回退），否则改端口后永远等旧端口超时
    let probe_ports = if def.id == "edge-conductor" {
        crate::ops::edge_config::effective_ports(std::path::Path::new(&def.install_dir), &def.ports)
    } else {
        def.ports.clone()
    };
    let ports = probe_ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join("/");
    let start = Instant::now();
    let mut next_progress = READY_PROGRESS_SECS;
    loop {
        if probe_ports
            .iter()
            .all(|p| health::check_tcp("127.0.0.1", *p, 800))
        {
            return Ok(());
        }
        let elapsed = start.elapsed().as_secs();
        if elapsed >= timeout_secs {
            if !procs.is_running(id) {
                // 进程已退出：退出码 + 输出尾部直接带回——JVM 报错
                // （类版本不兼容/配置解析失败/端口被占）当场可见。
                // 尾部 40 行：8 行只有栈底，异常标题（第一因）在更高处
                let code = procs
                    .last_exit_code(id)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "未知".into());
                let tail = ProcManager::spawn_log_tail(&def.install_dir, 40).join(" | ");
                return Err(format!(
                    "启动失败：进程已退出（exit={code}），端口 {ports} 未监听。进程输出尾部：{tail}（完整输出见 {}\\logs\\platform-spawn.log）",
                    def.install_dir
                ));
            }
            let tail = ProcManager::spawn_log_tail(&def.install_dir, 24).join(" | ");
            return Err(format!(
                "启动后未在超时内就绪：进程仍在运行但端口 {ports} 未监听（已等待 {timeout_secs}s）。进程输出尾部：{tail}"
            ));
        }
        if elapsed >= next_progress {
            crate::debug_log::entry(
                crate::ops::process::PROC_SCOPE,
                "INFO",
                "wait_ready",
                "progress",
                &format!("id={id} 已等待 {elapsed}s，端口 {ports} 未就绪，继续等待…"),
            );
            next_progress += READY_PROGRESS_SECS;
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

/// 启动前置：清掉占用服务端口的残留孤儿进程（上一轮停止未杀干净/用户手动
/// 关窗口/平台重启丢 pid）。端口空闲时无操作；重复点「启动」时相当于先清旧
/// 再起新（隐式重启语义，避免新旧进程抢端口双双失败）。
fn prepare_start(def: &ServiceDef) {
    // remote_only：端口在远程机器上，本机 netstat 查不到、也不能清
    // static_site：端口归宿主（dfy-ui 的 80 属于 nginx），清了会误杀宿主
    if def.remote_only || def.static_site {
        return;
    }
    if let Some(port) = def.ports.first() {
        crate::ops::process::kill_port_occupants(*port);
    }
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
            // 第三方依赖不归平台托管（由系统服务/容器运行时管理），启停一律跳过；
            // 静态托管（dfy-ui）无进程可启停，且 stop_deep 按端口兜底会把宿主
            // nginx 杀掉，同样跳过
            if def.third_party || def.static_site {
                return None;
            }
            // 停止不按 is_running 预过滤：它只认平台句柄，外部拉起的实例会被
            // 漏掉（「全部停止」停不掉外部进程）。stop_deep 内部按端口判定，
            // 端口空闲时是安全 no-op，多探测一次 netstat 无副作用
            let r = if start {
                prepare_start(def);
                procs.start(&id, &def.install_dir, &def.run_cmd)
            } else {
                // stop_deep：全部停止时对「外部拉起/句柄丢失」的服务按端口兜底强杀
                procs.stop_deep(&id, &def.ports)
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
        .filter(|d| d.is_daemon && !d.third_party && !d.static_site)
        .map(|d| d.id.as_str())
        .collect();
    match start_order(defs) {
        Ok(order) => order
            .into_iter()
            .filter(|id| daemon_ids.contains(id.as_str()))
            .filter_map(|id| {
                let def = defs.iter().find(|d| d.id == id)?;
                if !procs.is_running(&id) {
                    prepare_start(def);
                    if let Err(e) = procs.start(&id, &def.install_dir, &def.run_cmd) {
                        return Some(ActionResult {
                            id,
                            ok: false,
                            error: Some(e.to_string()),
                        });
                    }
                }
                let res = wait_ready(procs, &id, def, READY_TIMEOUT_SECS);
                Some(ActionResult {
                    id,
                    ok: res.is_ok(),
                    error: res.err(),
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
                    prepare_start(def);
                    if let Err(e) = procs.start(&sid, &def.install_dir, &def.run_cmd) {
                        return Some(ActionResult {
                            id: sid,
                            ok: false,
                            error: Some(e.to_string()),
                        });
                    }
                }
                let res = wait_ready(procs, &sid, def, READY_TIMEOUT_SECS);
                Some(ActionResult {
                    id: sid,
                    ok: res.is_ok(),
                    error: res.err(),
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
            remote_only: false,
            static_site: false,
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
