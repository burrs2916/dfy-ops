use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::server::AppState;
use crate::ops::ai;
use crate::ops::appdetect;
use crate::ops::diagnosis;
use crate::ops::environment;
use crate::ops::health;
use crate::ops::logs;
use crate::ops::orchestrator;
use crate::ops::process::ProcSnapshot;
use crate::ops::registry::{ServiceDef, ServiceKind};

const PROBE_TIMEOUT_MS: u64 = 400;

/// 服务运行态（三态，P0-2）。
///
/// - `managed`：由本平台启动，且 `ProcManager` 持有真实存活的进程句柄
///   （P1-4 已让 `is_running` 通过 `try_wait` 真实探活并回收僵尸）。
/// - `detected`：本平台未拉起，但端口探测到在监听，或按 `processNames` 探测到
///   本机进程（无端口桌面客户端的兜底通道）——服务由外部进程拉起或历史遗留。
///   属于「在运行但不在我们掌控内」。
/// - `down`：既无受管进程、端口也未监听。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Managed,
    Detected,
    Down,
    /// 第三方依赖（虚拟机/远程部署场景）：本机没有进程与端口，也未配置远程
    /// 地址——「设置」必须在「检测」之前，此时不判定 Down（那会误导用户以为
    /// 服务挂了），而是提示先到「远程服务」配置地址，配置后检测才有意义。
    #[serde(rename = "needs-config")]
    NeedsConfig,
}

/// 把同步阻塞段移交 tokio 的 blocking 线程池执行。
///
/// 必须这么做的原因：Axum handler 跑在 tokio worker 线程上，worker 数默认 = CPU 核数。
/// 而本模块的 handler 内部包含大量同步阻塞操作——
///   · 端口探测（check_tcp，每端口最长 800ms，前端 5s 轮询持续触发）
///   · 环境探测（Command::output 拉起 java/nginx 子进程）
///   · 依赖编排的就绪等待（wait_ready 内 thread::sleep，单服务最长 30s）
///   · 日志文件 / 配置文件的同步读写
/// 若在 handler 里直接同步执行，一次「全部启动」就能把 worker 池占满（30s × N 个服务），
/// 期间 /api/health 与前端状态轮询全部排队，用户看到的现象就是界面卡死。
/// blocking 池默认上限 512 线程，与 worker 池隔离，因此不会互相拖累。
async fn blocking<T, F>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .expect("blocking task 执行失败（内部 panic）")
}

/// 服务的实时状态（运行 + 健康）。
#[derive(Debug, Clone, Serialize)]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    #[serde(rename = "kind")]
    pub kind: ServiceKind,
    pub running: bool,
    #[serde(rename = "runState")]
    pub run_state: RunState,
    pub healthy: bool,
    pub ports: Vec<u16>,
    #[serde(rename = "dependsOn")]
    pub depends_on: Vec<String>,
    #[serde(rename = "isDaemon")]
    pub is_daemon: bool,
    /// 第三方依赖（VerneMQ/MinIO 等）：平台只检测不托管，前端据此隐藏启停按钮并分组展示。
    #[serde(rename = "thirdParty")]
    pub third_party: bool,
    /// 纯远程部署（Redis/VerneMQ/MinIO）：本机不安装，检测 = 托管端点可达性；
    /// 前端据此把「Windows 服务管理器」类提示换成远程承载语义。
    #[serde(rename = "remoteOnly")]
    pub remote_only: bool,
    /// 静态托管服务（dfy-ui：nginx 反代静态目录）：无独立进程，检测 = 本地
    /// 声明端口 TCP 探测；无启停按钮，不参与全部启停，前端据此隐藏启停入口。
    #[serde(rename = "staticSite")]
    pub static_site: bool,
    /// 「已安装」（Windows 卸载注册表判定）。三态语义：false = 确认未安装；
    /// true = 已安装或「不适用」（未声明 productNames 的服务一律 true，
    /// 维持既有「已停止/运行中」显示，不误报未安装）。
    #[serde(rename = "installed")]
    pub installed: bool,
    pub description: String,
    /// 服务的预期安装目录（Windows 路径，仅作展示与排障参考，平台不会自动创建/部署）。
    #[serde(rename = "installDir")]
    pub install_dir: String,
    /// 构建命令（若服务需要源码构建）。
    #[serde(rename = "buildCmd")]
    pub build_cmd: Option<String>,
    /// 启动命令。
    #[serde(rename = "runCmd")]
    pub run_cmd: String,
    /// 健康检查 URL（可选）。
    #[serde(rename = "healthUrl")]
    pub health_url: Option<String>,
    /// 日志文件路径列表。
    #[serde(rename = "logPaths")]
    pub log_paths: Vec<String>,
}

/// 第三方服务的远程检测目标：全部路由到 service_remote 的统一口径——
/// - Redis（remoteOnly）：端点在凭据文件 redis_credential.json；
/// - VerneMQ / MinIO（remoteOnly + thirdParty）：端点在 remote_endpoints.json
///   （「远程部署」配置面板统一保存）；
/// - GreptimeDB：随边缘计算捆绑本地化后，端点优先读边缘计算 config.toml
///   [greptimedb] addr（真实生效值），未安装边缘计算时回退 remote_endpoints.json。
///
/// 两者从未保存过地址时都返回 None → 状态转「待配置」，引导用户先完成设置，
/// 绝不拿默认值兜底探测（会误导），也绝不回退本机进程/端口检测
/// （服务跑在虚拟机/远程主机上时，本机检测必然误报 Down）。
fn remote_of(state: &AppState, def: &ServiceDef) -> Option<(String, u16)> {
    if def.remote_only || def.third_party {
        return crate::api::handlers_network::service_remote(state, &def.id);
    }
    None
}

fn to_status(
    def: &ServiceDef,
    managed: bool,
    snap: &ProcSnapshot,
    remote: Option<&(String, u16)>,
) -> ServiceStatus {
    // 受管 = 平台从安装目录拉起过，安装维度不适用；外部检测才判「装没装」；
    // 纯远程服务（remoteOnly）跳过本机进程/端口检测，检测 = 托管端点 TCP 可达。
    let (run_state, ports, healthy, installed) = if let Some((host, port)) = remote {
        let ok = health::check_tcp(host, *port, PROBE_TIMEOUT_MS);
        (
            if ok { RunState::Detected } else { RunState::Down },
            vec![*port],
            ok,
            true,
        )
    } else if managed {
        // edge-conductor：端口展示同样跟随 config.toml 实际值（配置中心可改）
        let ports = if def.id == "edge-conductor" {
            crate::ops::edge_config::effective_ports(
                std::path::Path::new(&def.install_dir),
                &def.ports,
            )
        } else {
            def.ports.clone()
        };
        (RunState::Managed, ports, managed_healthy(def), true)
    } else {
        detect_external(def, snap)
    };
    // 先设置、后检测：第三方服务本机既无进程也无端口、又没有配置远程地址时，
    // 检测得不出「未运行」的结论（它只说明「没在检测目标上」）——转为待配置态，
    // 由前端引导用户先完成地址设置。
    let run_state = if run_state == RunState::Down && def.third_party && remote.is_none() {
        RunState::NeedsConfig
    } else {
        run_state
    };
    ServiceStatus {
        id: def.id.clone(),
        name: def.name.clone(),
        kind: def.kind.clone(),
        running: matches!(run_state, RunState::Managed | RunState::Detected),
        run_state,
        healthy,
        ports,
        depends_on: def.depends_on.clone(),
        is_daemon: def.is_daemon,
        third_party: def.third_party,
        remote_only: def.remote_only,
        static_site: def.static_site,
        installed,
        description: def.description.clone(),
        install_dir: def.install_dir.clone(),
        build_cmd: def.build_cmd.clone(),
        run_cmd: def.run_cmd.clone(),
        health_url: def.health_url.clone(),
        log_paths: def.log_paths.clone(),
    }
}

/// 受管服务（本平台拉起）的健康判定：就绪等待本来就以配置端口为目标，沿用它。
fn managed_healthy(def: &ServiceDef) -> bool {
    if let Some(url) = &def.health_url {
        health::check_http(url, PROBE_TIMEOUT_MS)
    } else if def.ports.is_empty() {
        // 无端口的桌面客户端：是否「健康」直接等同于「在运行」
        true
    } else {
        // edge-conductor 的端口可被配置中心修改——健康探测以 config.toml
        // 实际端口为准，否则改端口后永远显示「不健康」
        let ports = if def.id == "edge-conductor" {
            crate::ops::edge_config::effective_ports(
                std::path::Path::new(&def.install_dir),
                &def.ports,
            )
        } else {
            def.ports.clone()
        };
        ports
            .iter()
            .all(|p| health::check_tcp("127.0.0.1", *p, PROBE_TIMEOUT_MS))
    }
}

/// 外部运行服务的检测：**进程是事实来源，端口由进程反查**。
///
/// 平台不规定服务用什么端口——按 `processNames`（运行器特征，如 `ng serve`、
/// `dfy-server.jar`）定位 PID，再反查这些 PID 实际监听的端口：
/// 反查到什么就上报什么（前端展示与 healthy 判定均以实际端口为准）。
/// 仅当未配置进程名或未命中时，才回退到按配置端口探测（兼容 nginx 多实例这类
/// 不适合按进程名区分的部署形态）。
///
/// 「装没装」独立判定（appdetect，Windows 读卸载注册表，与部署路径无关）：
/// - 声明了 `productNames` → 注册表实查，未装如实上报 installed=false；
/// - 未声明 → true（不适用，维持既有显示逻辑）。
///
/// `snap`：调用方采集一次的全量进程快照（批量检测的关键——逐服务起子进程
/// 在 Windows 上要 O(N) 次 tasklist/netstat/PowerShell，快照摊薄为 O(1)）。
///
/// 返回（状态，端口，健康，已安装）。
fn detect_external(def: &ServiceDef, snap: &ProcSnapshot) -> (RunState, Vec<u16>, bool, bool) {
    // 「装没装」与「跑没跑」是两个独立事实：注册表安装记录毫秒级可得，
    // 且不受进程检测的任何启发式影响——没装就是没装，别让兜底探测把它洗成「正常」。
    let installed = if def.product_names.is_empty() {
        true
    } else {
        let exe_names: Vec<String> = def
            .process_names
            .iter()
            .map(|n| n.trim_end_matches(".exe").to_string())
            .collect();
        appdetect::is_installed(&def.product_names, &exe_names)
    };
    // 无端口预期（桌面客户端）：只信镜像名，禁用命令行兜底——查询自身与
    // IDE 等的命令行可能含目标名，会把「没装没跑」误判成「检测到在运行」。
    let mut pids: Vec<u32> = def
        .process_names
        .iter()
        .flat_map(|n| snap.pids_for(n, !def.ports.is_empty()))
        .collect();
    pids.sort_unstable();
    pids.dedup();
    if !pids.is_empty() {
        let discovered = snap.ports_of(&pids);
        if !discovered.is_empty() {
            // 进程在且端口在监听：以实际监听端口为准
            return (RunState::Detected, discovered, true, installed);
        }
        // 进程在但没有监听端口：无端口预期（桌面客户端）视为健康；
        // 有端口预期则回退按配置端口确认（lsof 不可用时的兜底）。
        let healthy = if def.ports.is_empty() {
            true
        } else {
            def.ports
                .iter()
                .all(|p| health::check_tcp("127.0.0.1", *p, PROBE_TIMEOUT_MS))
        };
        return (RunState::Detected, def.ports.clone(), healthy, installed);
    }
    // 兜底：未配置进程名或未命中 → 按配置端口探测。
    // 一轮探测同时得出 port_up（any）与 healthy（all）：此前同一组端口
    // 各探一遍，端口不通时 800ms 超时 × 2，全量串行下白白翻倍。
    let results: Vec<bool> = def
        .ports
        .iter()
        .map(|p| health::check_tcp("127.0.0.1", *p, PROBE_TIMEOUT_MS))
        .collect();
    let healthy = if let Some(url) = &def.health_url {
        health::check_http(url, PROBE_TIMEOUT_MS)
    } else if def.ports.is_empty() {
        false
    } else {
        results.iter().all(|&b| b)
    };
    let state = if results.iter().any(|&b| b) {
        RunState::Detected
    } else {
        RunState::Down
    };
    (state, def.ports.clone(), healthy, installed)
}

fn find_def<'a>(state: &'a AppState, id: &str) -> Option<&'a ServiceDef> {
    state.registry.iter().find(|d| d.id == id)
}

// ===== 环境修复（java 检查项的分级修复入口） =====

#[derive(Debug, Deserialize)]
pub struct EnvRepairInput {
    #[serde(rename = "serviceId")]
    pub service_id: String,
    #[serde(rename = "checkId")]
    pub check_id: String,
}

/// POST /api/env/repair —— 分级修复决策链：
/// 1. PATH java 已达标 → action=none（无需修复）；
/// 2. 本机已有达标 JDK（PATH 不达标）→ action=configure（返回配置指引，不擅自改）；
/// 3. 纯净机无任何达标 JDK → action=install（启动自动安装：下载 Temurin MSI →
///    UAC 提权静默安装并配置环境变量 → 复探验证），前端轮询 /api/env/repair/status。
pub async fn start_env_repair(
    State(state): State<Arc<AppState>>,
    Json(input): Json<EnvRepairInput>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if input.check_id != "java" {
        return Ok(Json(serde_json::json!({
            "ok": false,
            "error": "该检查项暂不支持自动修复，请按修复建议手动处理"
        })));
    }
    let def = find_def(&state, &input.service_id)
        .ok_or(StatusCode::NOT_FOUND)?
        .clone();
    let expected = def.java_major.unwrap_or(17);

    // 第一事实：PATH 上的 java（run_cmd 实际执行者）
    let path_major = blocking(crate::ops::environment::probe_java_version).await;
    if let Some((major, _, _)) = path_major {
        if major >= expected {
            return Ok(Json(serde_json::json!({
                "ok": true,
                "action": "none",
                "message": format!("PATH 中的 java 已满足 JDK {expected}+，无需修复")
            })));
        }
    }

    // 本机已有达标 JDK：配置问题而非缺失，给指引不代改
    let compatible = blocking(move || crate::ops::env_repair::find_compatible(expected)).await;
    if let Some(j) = compatible {
        return Ok(Json(serde_json::json!({
            "ok": true,
            "action": "configure",
            "home": j.home,
            "message": format!(
                "本机已有达标 JDK {}（{}）：无需安装，请将其 bin 目录加入 PATH 或设为 JAVA_HOME",
                j.version_label(),
                j.home
            )
        })));
    }

    // 纯净机：启动自动安装（Windows：下载+UAC 提权静默装；其他平台返回指引）
    match crate::ops::env_repair::start_jdk_install(
        expected,
        state.env_cache.clone(),
        state.repair.clone(),
    ) {
        Ok(_) => Ok(Json(serde_json::json!({
            "ok": true,
            "action": "install",
            "message": format!("开始自动安装 Eclipse Temurin {expected} JRE（下载 → 管理员确认 → 验证），进度见下方任务状态")
        }))),
        Err(e) => Ok(Json(serde_json::json!({"ok": false, "error": e}))),
    }
}

/// GET /api/env/repair/status —— 前端轮询修复任务进度。
pub async fn env_repair_status(
    State(state): State<Arc<AppState>>,
) -> Json<crate::ops::env_repair::RepairJob> {
    Json(state.repair.snapshot())
}

pub async fn server_health(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "services": state.registry.len(),
        "api": "dfy-ops local ops",
    }))
}

// ===== 应用诊断（debug.log 自证） =====

/// 应用诊断：真实版本号 + debug.log 注册路径与存在性。
/// 应用在「设置 → 关于」里自己报告日志写在哪、生成没有——用户不用再翻目录猜。
pub async fn diagnostics() -> Json<serde_json::Value> {
    let logs: Vec<serde_json::Value> = crate::debug_log::registered_paths()
        .iter()
        .map(|p| {
            serde_json::json!({
                "path": p.display().to_string(),
                "exists": p.exists(),
            })
        })
        .collect();
    Json(serde_json::json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "log_paths": logs,
    }))
}

/// 在系统文件管理器中打开 debug.log 所在目录。
/// 优先选第一个已存在的日志文件；都不存在时打开第一个注册路径的父目录并顺带建目录。
pub async fn diagnostics_open_log_dir() -> Json<serde_json::Value> {
    let paths = crate::debug_log::registered_paths();
    let dir = paths
        .iter()
        .find(|p| p.exists())
        .or_else(|| paths.first())
        .and_then(|p| p.parent())
        .map(|d| d.to_path_buf());
    let Some(dir) = dir else {
        return Json(serde_json::json!({ "ok": false, "error": "日志路径尚未注册" }));
    };
    let _ = std::fs::create_dir_all(&dir);
    match open_in_file_manager(&dir) {
        Ok(_) => Json(serde_json::json!({ "ok": true, "dir": dir.display().to_string() })),
        Err(e) => Json(serde_json::json!({
            "ok": false,
            "error": format!("打开失败：{e}"),
            "dir": dir.display().to_string(),
        })),
    }
}

/// 跨平台调用系统文件管理器打开目录。
fn open_in_file_manager(
    dir: &std::path::Path,
) -> std::io::Result<std::process::Child> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(dir).spawn()
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(dir).spawn()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(dir).spawn()
    }
}

/// 打开文件管理器并【定位选中】指定文件（监控中心「打开目录」用：不仅开到
/// 日志所在目录，还要高亮该日志文件本身，免得用户在一堆日志里再找一遍）。
/// - Windows：`explorer /select,"<完整路径>"`（注意 /select, 与路径是一个参数，
///   路径必须反斜杠原生格式，否则资源管理器只开到默认页）
/// - macOS：Finder 反显（open -R）
/// - 其他 Unix：退化为打开父目录（无通用反显命令）
fn open_in_file_manager_select(file: &std::path::Path) -> std::io::Result<std::process::Child> {
    #[cfg(target_os = "windows")]
    {
        let arg = format!("/select,{}", file.display());
        std::process::Command::new("explorer").arg(arg).spawn()
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg("-R").arg(file).spawn()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let dir = file.parent().unwrap_or(file);
        std::process::Command::new("xdg-open").arg(dir).spawn()
    }
}

/// 全量状态探测：一次进程快照 + 并行逐服务检测。
///
/// 服务间无共享可变状态，各自的 TCP 探测/注册表查询完全独立——此前串行
/// 执行，端口不通的服务每个要吃 800ms 超时，9 个服务叠加数秒；并行后
/// 总耗时 ≈ 最慢单服务。快照把 Windows 的 tasklist/netstat/PowerShell
/// 子进程调用摊薄为固定 2-3 次，与注册表服务数无关。
fn statuses(state: &AppState, snap: &ProcSnapshot) -> Vec<ServiceStatus> {
    std::thread::scope(|s| {
        let handles: Vec<_> = state
            .registry
            .iter()
            .map(|d| {
                let remote = remote_of(state, d);
                s.spawn(move || to_status(d, state.procs.is_running(&d.id), snap, remote.as_ref()))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("服务状态探测线程 panic"))
            .collect()
    })
}

pub async fn list_services(State(state): State<Arc<AppState>>) -> Json<Vec<ServiceStatus>> {
    let out = blocking(move || {
        let snap = ProcSnapshot::capture();
        statuses(&state, &snap)
    })
    .await;
    Json(out)
}

pub async fn get_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<ServiceStatus>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?.clone();
    let st = state.clone();
    let status = blocking(move || {
        let snap = ProcSnapshot::capture();
        let remote = remote_of(&st, &def);
        to_status(&def, st.procs.is_running(&id), &snap, remote.as_ref())
    })
    .await;
    Ok(Json(status))
}

pub async fn start_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    if find_def(&state, &id).is_none() {
        return Json(serde_json::json!({"ok": false, "error": "service not found"}));
    }
    // 静态托管（dfy-ui）：无进程可启动，nginx 已代为承载
    if find_def(&state, &id).map(|d| d.static_site).unwrap_or(false) {
        return Json(serde_json::json!({"ok": false,
            "error": "静态托管服务：由 nginx 反代承载，无需启动；检测按端口可达性判定"}));
    }
    let id_for_result = id.clone();
    let results =
        blocking(move || orchestrator::start_with_deps(&state.procs, &state.registry, &id)).await;
    Json(serde_json::json!({
        "ok": results.iter().all(|r| r.ok),
        "id": id_for_result,
        "details": results,
    }))
}

pub async fn stop_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    // 克隆声明端口后再进 blocking：stop_deep 无句柄时按端口强杀外部拉起的实例
    let def_ports = match find_def(&state, &id) {
        Some(d) if d.static_site => {
            // 关键守卫：dfy-ui 声明的 80 就是 nginx 的端口，stop_deep 按端口
            // 兜底会把宿主 nginx 整个杀掉——静态托管服务一律拒绝停止
            return Json(serde_json::json!({"ok": false,
                "error": "静态托管服务：无独立进程，无需停止（如需维护请停 nginx）"}));
        }
        Some(d) => d.ports.clone(),
        None => return Json(serde_json::json!({"ok": false, "error": "service not found"})),
    };
    let id_for_result = id.clone();
    let r = blocking(move || state.procs.stop_deep(&id_for_result, &def_ports)).await;
    Json(serde_json::json!({
        "ok": r.is_ok(),
        "id": id,
        "error": r.err().map(|e| e.to_string()),
    }))
}

pub async fn restart_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<serde_json::Value> {
    let def_ports = match find_def(&state, &id) {
        Some(d) if d.static_site => {
            // 同 stop_service 守卫：restart = stop_deep + start，会误杀宿主 nginx
            return Json(serde_json::json!({"ok": false,
                "error": "静态托管服务：无独立进程，无需重启（更新前端请直接覆盖部署目录）"}));
        }
        Some(d) => d.ports.clone(),
        None => return Json(serde_json::json!({"ok": false, "error": "service not found"})),
    };
    let id_for_result = id.clone();
    // stop + start 合并在同一个 blocking 任务里：stop 在 Windows 上要走 taskkill，
    // start 内含最长 30s 的就绪等待，两者都是同步阻塞，一起挪出 worker 池。
    // stop 用 stop_deep：平台重启过/服务外部拉起时无句柄，按端口兜底强杀，
    // 否则旧进程占着端口，紧跟着的 start 必然 bind 失败。
    let results = blocking(move || {
        let _ = state.procs.stop_deep(&id, &def_ports);
        orchestrator::start_with_deps(&state.procs, &state.registry, &id)
    })
    .await;
    Json(serde_json::json!({
        "ok": results.iter().all(|r| r.ok),
        "id": id_for_result,
        "details": results,
    }))
}

/// 按依赖拓扑顺序启动全部常驻服务。
pub async fn start_all(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<orchestrator::ActionResult>> {
    Json(blocking(move || orchestrator::start_all(&state.procs, &state.registry)).await)
}

/// 按逆依赖顺序停止全部服务。
pub async fn stop_all(State(state): State<Arc<AppState>>) -> Json<Vec<orchestrator::ActionResult>> {
    Json(blocking(move || orchestrator::stop_all(&state.procs, &state.registry)).await)
}

pub async fn build_service(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?.clone();
    let Some(cmd) = def.build_cmd.clone() else {
        return Ok(Json(
            serde_json::json!({"ok": false, "error": "该服务未配置 buildCmd"}),
        ));
    };
    let log_path = PathBuf::from(&def.install_dir)
        .join("build.log")
        .to_string_lossy()
        .to_string();
    let result = blocking(move || state.procs.build(&def.install_dir, &cmd, &log_path)).await;
    Ok(Json(match result {
        Ok(_) => serde_json::json!({"ok": true, "id": id}),
        Err(e) => serde_json::json!({"ok": false, "error": e.to_string()}),
    }))
}

/// 读取指定服务的构建日志（前端轮询以展示构建进度）。
pub async fn build_log(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    let path = PathBuf::from(&def.install_dir).join("build.log");
    let text = blocking(move || {
        if path.exists() {
            std::fs::read_to_string(&path).unwrap_or_default()
        } else {
            String::new()
        }
    })
    .await;
    Ok(Json(serde_json::json!({"id": id, "logs": text})))
}

pub async fn service_logs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<LogQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    let log_paths = def.log_paths.clone();
    let want = q.path.clone();
    let id_ref = id.clone();
    let text = blocking(move || match want {
        // 监控中心选中具体文件：只读该文件。白名单校验——路径必须是
        // registry.log_paths 声明的文件，或声明目录下的 *.log，防止越权读盘
        Some(p) => {
            let allowed = log_paths.iter().any(|lp| {
                let lpb = std::path::Path::new(lp);
                let pp = std::path::Path::new(&p);
                if lpb.is_dir() {
                    pp.starts_with(lpb)
                        && pp
                            .file_name()
                            .map(|n| crate::ops::logs::is_log_file_name(&n.to_string_lossy()))
                            .unwrap_or(false)
                } else {
                    lpb == pp
                }
            });
            if !allowed {
                return format!("[拒绝访问] 该文件不在服务 {id_ref} 的日志声明范围内");
            }
            logs::tail_many(std::slice::from_ref(&p), 200)
        }
        None => logs::tail_many(&log_paths, 200),
    })
    .await;
    Ok(Json(serde_json::json!({"id": id, "logs": text})))
}

#[derive(Deserialize)]
pub struct LogQuery {
    /// 可选：只读这一个日志文件（监控中心选中某文件时传）
    path: Option<String>,
}

/// 监控中心：全部服务的日志文件清单（统一管理的列表数据源）。
/// 遍历 registry.log_paths：文件直接收录，目录枚举其下 *.log；
/// 附运行状态、大小、最后修改时间——前端按修改时间倒序展示，
/// 「有没有在动」一眼可辨（日志停更通常意味着进程卡死或已退出）。
pub async fn log_files(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let s = state.clone();
    let rows = blocking(move || {
        let snap = ProcSnapshot::capture();
        let deps = dep_states(&s, &snap);
        let running: HashMap<String, bool> = deps
            .iter()
            .map(|(id, d)| (id.clone(), d.running))
            .collect();
        s.registry
            .iter()
            // 只收录「本地可收集日志」的服务：log_paths 为空 = 纯远程或无日志
            // （Redis/VerneMQ/MinIO 等远程部署，其自身日志无法在运维端收集，
            // 2026-11-04 用户定案：远程服务不进监控中心，只运维本地）
            .filter(|d| !d.log_paths.is_empty())
            .map(|d| {
                let mut files: Vec<serde_json::Value> = Vec::new();
                for p in &d.log_paths {
                    let pb = std::path::Path::new(p);
                    if pb.is_dir() {
                        if let Ok(rd) = std::fs::read_dir(pb) {
                            for e in rd.flatten() {
                                let ep = e.path();
                                let fname = e.file_name().to_string_lossy().to_string();
                                if !crate::ops::logs::is_log_file_name(&fname) {
                                    continue;
                                }
                                let mut m = log_file_meta(&ep);
                                // 带日期后缀的轮转文件按归档件展示：
                                // 计入归档占用汇总与一键清理（与 logback 归档同待遇）
                                if !fname.ends_with(".log") {
                                    m["derived"] = serde_json::Value::Bool(true);
                                }
                                files.push(m);
                            }
                        }
                    } else if pb.is_file() {
                        files.push(log_file_meta(pb));
                        // 归档/衍生文件一并收录：logback 按天滚动出的
                        // dfy-server.log.2026-11-04.0.gz、轮转的 .prev.log、
                        // 平台备份的 .bak-*。长年运行归档会积累得非常大，
                        // 运维必须在监控中心里看到并可清理（2026-11-04 定案）
                        if let (Some(dir), Some(name)) = (pb.parent(), pb.file_name()) {
                            if let Ok(rd) = std::fs::read_dir(dir) {
                                let prefix = format!("{}.", name.to_string_lossy());
                                for e in rd.flatten() {
                                    let ep = e.path();
                                    if !ep.is_file() {
                                        continue;
                                    }
                                    let en = e.file_name();
                                    let en = en.to_string_lossy();
                                    if en.starts_with(&prefix) {
                                        let mut m = log_file_meta(&ep);
                                        m["derived"] = serde_json::Value::Bool(true);
                                        files.push(m);
                                    }
                                }
                            }
                        }
                    }
                }
                files.sort_by(|a, b| {
                    b["modified"]
                        .as_u64()
                        .unwrap_or(0)
                        .cmp(&a["modified"].as_u64().unwrap_or(0))
                });
                // 归档占用汇总：运维看「积了多少、该不该清」，一键清理看这里
                let (archive_count, archive_bytes) = files
                    .iter()
                    .filter(|f| f["derived"].as_bool().unwrap_or(false))
                    .fold((0u64, 0u64), |(n, b), f| {
                        (n + 1, b + f["size"].as_u64().unwrap_or(0))
                    });
                serde_json::json!({
                    "id": d.id,
                    "name": d.name,
                    "running": running.get(&d.id).copied().unwrap_or(false),
                    "archiveCount": archive_count,
                    "archiveBytes": archive_bytes,
                    "files": files,
                })
            })
            .collect::<Vec<_>>()
    })
    .await;
    Json(serde_json::json!({ "services": rows }))
}

fn log_file_meta(p: &std::path::Path) -> serde_json::Value {
    let (size, modified) = match std::fs::metadata(p) {
        Ok(m) => (
            m.len(),
            m.modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0),
        ),
        Err(_) => (0, 0),
    };
    serde_json::json!({
        "path": p.to_string_lossy(),
        "name": p.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or_default(),
        "size": size,
        "modified": modified,
    })
}

/// 校验 path 是否在某服务的日志声明白名单内（与 service_logs ?path= 同一规则）。
/// 返回 (允许, 服务的 install_dir)——install_dir 用于日志目录兜底定位。
fn log_path_allowed(def: &crate::ops::registry::ServiceDef, p: &str) -> bool {
    def.log_paths.iter().any(|lp| {
        let lpb = std::path::Path::new(lp);
        let pp = std::path::Path::new(p);
        if lpb.is_dir() {
            pp.starts_with(lpb) && pp.extension().map(|x| x == "log").unwrap_or(false)
        } else {
            // 精确匹配声明文件，或其归档/备份衍生（同目录、以「声明名.」开头）：
            // dfy-server.log.2026-11-04.0.gz / .prev.log / .bak-* 与主日志同属
            // 维护范围，删除前同样受白名单约束
            lpb == pp
                || (pp.parent() == lpb.parent()
                    && pp
                        .file_name()
                        .zip(lpb.file_name())
                        .map(|(pf, lf)| {
                            let pf = pf.to_string_lossy();
                            let lf = lf.to_string_lossy();
                            pf.starts_with(&format!("{lf}.")) && !pf.ends_with(".pid")
                        })
                        .unwrap_or(false))
        }
    })
}

#[derive(Deserialize)]
pub struct LogCleanupBody {
    /// 保留最近 N 天的归档；None/0 = 全部清理。主日志文件永不删除。
    keep_days: Option<u32>,
}

/// 归档管理：批量清理某服务的归档/衍生日志（滚动 gz、.prev、.bak-*）。
/// 只动白名单内的衍生文件，正在写入的主日志永不删；运行中的服务不受影响。
pub async fn log_cleanup_archives(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(body): axum::Json<LogCleanupBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // 克隆声明路径后再进 blocking 闭包：find_def 返回的引用借自 state，
    // 闭包要求 'static，直接捕获 def 编译不过
    let log_paths: Vec<String> = find_def(&state, &id)
        .ok_or(StatusCode::NOT_FOUND)?
        .log_paths
        .clone();
    let keep_days = body.keep_days.unwrap_or(0);
    let cutoff = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0))
    .saturating_sub(keep_days as u64 * 86400);
    let result = blocking(move || {
        let mut removed = 0u64;
        let mut bytes = 0u64;
        let mut errors: Vec<String> = Vec::new();
        for lp in &log_paths {
            let lpb = std::path::Path::new(lp);
            if lpb.is_dir() {
                continue; // 目录声明（edge-conductor）不做批量清理，避免误删范围过大
            }
            let (dir, name) = match (lpb.parent(), lpb.file_name()) {
                (Some(d), Some(n)) => (d.to_path_buf(), n.to_string_lossy().to_string()),
                _ => continue,
            };
            let prefix = format!("{name}.");
            let rd = match std::fs::read_dir(&dir) {
                Ok(r) => r,
                Err(_) => continue,
            };
            for e in rd.flatten() {
                let ep = e.path();
                if !ep.is_file() {
                    continue;
                }
                let en = e.file_name().to_string_lossy().to_string();
                if !en.starts_with(&prefix) || en.ends_with(".pid") {
                    continue;
                }
                let size = std::fs::metadata(&ep).map(|m| m.len()).unwrap_or(0);
                let mtime = std::fs::metadata(&ep)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                if keep_days > 0 && mtime >= cutoff {
                    continue; // 保留期内，跳过
                }
                match std::fs::remove_file(&ep) {
                    Ok(()) => {
                        removed += 1;
                        bytes += size;
                    }
                    Err(e) => errors.push(format!("{en}: {e}")),
                }
            }
        }
        (removed, bytes, errors)
    })
    .await;
    let (removed, bytes, errors) = result;
    Ok(Json(serde_json::json!({
        "ok": errors.is_empty(),
        "removed": removed,
        "bytes": bytes,
        "errors": errors,
    })))
}

#[derive(Deserialize)]
pub struct LogMaintainBody {
    path: String,
}

/// 日志维护：备份（复制为 <原名>.bak-YYYYMMDD-HHMMSS）。
/// 日志不断增长，运维在删除/轮转前先留一份现场。
pub async fn log_backup(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(body): axum::Json<LogMaintainBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    if !log_path_allowed(def, &body.path) {
        return Ok(Json(serde_json::json!({"ok": false, "error": "该文件不在此服务的日志声明范围内"})));
    }
    let src = body.path.clone();
    let result = blocking(move || {
        let sp = std::path::Path::new(&src);
        if !sp.is_file() {
            return Err("日志文件不存在".to_string());
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // 时间戳 → 本地时间格式（简单按 secs 格式化，交付机本地时区由前端展示负责）
        let (y, mo, d, h, mi, s) = unix_to_local(ts);
        let name = sp
            .file_name()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_else(|| "log".into());
        let dst = sp.with_file_name(format!("{name}.bak-{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}"));
        std::fs::copy(sp, &dst)
            .map(|_| format!("{}", dst.display()))
            .map_err(|e| format!("备份失败：{e}"))
    })
    .await;
    match result {
        Ok(dst) => Ok(Json(serde_json::json!({"ok": true, "backup": dst}))),
        Err(e) => Ok(Json(serde_json::json!({"ok": false, "error": e}))),
    }
}

/// 尝试删除文件，带「进程锁持有」兜底：
/// 1) 直接 remove_file
/// 2) 失败且是 "being used" 类错误 → 调 kill_holders 强杀持有该路径的进程
/// 3) 等 300ms → 再删
/// 4) 还失败 → 返回带原始错误的 Err
///
/// 传入的 `service_id`/`ports` 用于兜底：kill_holders 在交付机没有
/// handle.exe/LSOF 时，退化为 stop_deep 该服务（logback 持有 dfy-server.log
/// 的文件句柄是最常见的占用来源）。
fn remove_file_with_fallback(path: &str, service_id: &str, ports: &[u16]) -> Result<(), String> {
    let sp = std::path::Path::new(path);
    if !sp.is_file() {
        return Err("日志文件不存在".to_string());
    }
    match std::fs::remove_file(sp) {
        Ok(()) => return Ok(()),
        Err(e) => {
            // 锁定判定：1.83 才有 std::io::ErrorKind::ResourceBusy，我们 MSRV 1.77
            // 靠错误字符串匹配更靠谱（Windows "being used / denied"，Unix EBUSY 文本也会走到这里）
            let msg = e.to_string().to_lowercase();
            let is_locked = msg.contains("being used")
                || msg.contains("denied")
                || msg.contains("busy");
            if !is_locked {
                return Err(format!("删除失败：{e}"));
            }
            crate::debug_log::entry(
                crate::ops::process::PROC_SCOPE,
                "WARN",
                "log_delete",
                "file_locked",
                &format!("path={path} err={e}，尝试 kill 占用进程"),
            );
        }
    }
    // 兜底：kill 掉声明端口上的占用进程（即持有该文件句柄的 java/nginx）
    // kill_port_occupants 会强杀 taskkill /T /F，文件句柄随之释放
    for p in ports {
        let _ = crate::ops::process::kill_port_occupants(*p);
    }
    // 等 300ms 让 OS 真正释放文件句柄（Windows NTFS 有短暂延迟）
    std::thread::sleep(std::time::Duration::from_millis(300));
    std::fs::remove_file(sp).map_err(|e| format!("删除失败（文件被其他进程占用，已尝试强杀 {service_id} 但仍锁着：{e}）"))
}

/// 日志维护：删除日志文件。正在写该文件的服务（logback/nginx）会自动重建，
/// 不影响运行；删除前由前端二次确认。被进程占用时自动 stop_deep 兜底。
pub async fn log_delete(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(body): axum::Json<LogMaintainBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    if !log_path_allowed(def, &body.path) {
        return Ok(Json(serde_json::json!({"ok": false, "error": "该文件不在此服务的日志声明范围内"})));
    }
    let path = body.path.clone();
    let sid = id.clone();
    let ports = def.ports.clone();
    let result = blocking(move || remove_file_with_fallback(&path, &sid, &ports)).await;
    match result {
        Ok(()) => Ok(Json(serde_json::json!({"ok": true}))),
        Err(e) => Ok(Json(serde_json::json!({"ok": false, "error": e}))),
    }
}

/// 日志维护：清空日志文件内容（截断为 0 字节）。
/// 用途：文件被进程占用删不掉时的兜底——服务（logback/nginx）持有句柄通常是
/// 共享写模式，截断一般能成功；即使个别句柄不允许写，也如实报错。
/// 与删除不同：文件本体保留、inode/句柄不动，服务无需重建文件，运行不受影响。
pub async fn log_clear(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::Json(body): axum::Json<LogMaintainBody>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    if !log_path_allowed(def, &body.path) {
        return Ok(Json(serde_json::json!({"ok": false, "error": "该文件不在此服务的日志声明范围内"})));
    }
    let path = body.path.clone();
    let result = blocking(move || {
        let sp = std::path::Path::new(&path);
        let before = std::fs::metadata(sp).map(|m| m.len()).unwrap_or(0);
        // OpenOptions write + truncate：不重建文件，只把长度归零
        std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(sp)
            .map_err(|e| {
                let msg = e.to_string().to_lowercase();
                if msg.contains("being used") || msg.contains("denied") || msg.contains("busy") {
                    "文件被其他进程以独占方式锁定，无法清空（只能等占用进程退出）".to_string()
                } else {
                    format!("清空失败：{e}")
                }
            })
            .map(|_| before)
    })
    .await;
    match result {
        Ok(before) => {
            crate::debug_log::entry(
                crate::ops::process::PROC_SCOPE,
                "INFO",
                "log_clear",
                "clear",
                &format!("path={} freed={before}", body.path),
            );
            Ok(Json(serde_json::json!({"ok": true, "freed": before})))
        }
        Err(e) => Ok(Json(serde_json::json!({"ok": false, "error": e}))),
    }
}

/// unix secs → (年,月,日,时,分,秒) 本地时区。chrono 未引入，用 libc localtime_r
/// 代价也不小——直接用 std 无法本地化，此处用 UTC+8 硬偏移（交付机固定在国内）。
fn unix_to_local(ts: u64) -> (u32, u32, u32, u32, u32, u32) {
    let ts = ts + 8 * 3600;
    let days = (ts / 86400) as i64;
    let rem = ts % 86400;
    let (h, mi, s) = ((rem / 3600) as u32, ((rem % 3600) / 60) as u32, (rem % 60) as u32);
    // civil_from_days（Howard Hinnant 算法）
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y } as u32;
    (y, m, d, h, mi, s)
}

pub async fn diagnosis(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    // 阶段一（阻塞）：端口探测 + 子进程环境探测 + 规则诊断，整体挪到 blocking 池，
    // 避免探测耗时把 worker 线程占住（这是诊断接口里最重的一段）。
    // 探测内部并行化（快照 + thread::scope + ProbeCache），与 /api/env 同一套优化。
    let llm_state = state.clone();
    let (status, env_reports, findings) = blocking(move || {
        // 依赖检查视图：同一份「进程是事实来源」的状态，同时驱动服务状态与环境/依赖检查。
        let snap = ProcSnapshot::capture();
        let deps = dep_states(&state, &snap);
        let status: HashMap<String, (bool, bool)> = deps
            .iter()
            .map(|(id, dep)| (id.clone(), (dep.running, dep.healthy)))
            .collect();
        // 环境检查：探测各服务的运行时依赖 + requires 声明 + dependsOn 依赖，并纳入诊断。
        let cache = environment::ProbeCache::default();
        let env_reports: Vec<environment::ServiceEnvReport> = std::thread::scope(|s| {
            // move 闭包捕获引用本身（Copy），避免把 deps/cache 整体 move 进闭包
            let deps = &deps;
            let cache = &cache;
            let handles: Vec<_> = state
                .registry
                .iter()
                .map(|d| s.spawn(move || environment::check_service_env_with(d, deps, Some(cache))))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("环境检查线程 panic"))
                .collect()
        });
        let findings = diagnosis::diagnose(&state.registry, &status, &env_reports);
        (status, env_reports, findings)
    })
    .await;

    // 阶段二（异步）：LLM 增强层 —— 按「模型 → 接入点 → 供应商」动态解析调用参数，
    // 未配置时退化为规则结果。网络请求本身是异步的，不占用 worker。
    let mut llm_summary: Option<String> = None;
    let mut llm_note: Option<String> = None;
    let target = llm_state.ai_models.lock().unwrap().active();
    if let Some(t) = target {
        let prompt = format!(
            "你是本地运维助手。以下是 Windows 主机上 DFY 系列服务的诊断数据，请用简体中文给出根因分析与处置建议：\n服务状态：{}\n环境检查：{}\n发现项：{}",
            serde_json::to_string(&status).unwrap_or_default(),
            serde_json::to_string(&env_reports).unwrap_or_default(),
            serde_json::to_string(&findings).unwrap_or_default()
        );
        match ai::analyze(&t, prompt).await {
            Ok(s) => llm_summary = Some(s),
            Err(e) => llm_note = Some(e),
        }
    } else {
        llm_note = Some("AI 模型未配置：请在「AI助手」添加供应商并接入点与模型".into());
    }

    Json(serde_json::json!({
        "status": status,
        "envs": env_reports,
        "findings": findings,
        "llmSummary": llm_summary,
        "llmNote": llm_note,
    }))
}

/// 构建依赖检查视图：遍历注册表，把每个服务的实时状态（进程级检测，事实来源；
/// remoteOnly 服务 = 托管端点可达性）汇总为 DepState 映射，供环境检查的
/// dependsOn 段消费。依赖端口沿用声明值（DepState.ports 仅作展示），
/// 实际端口已包含在 statuses 的检测里。
fn dep_states(state: &AppState, snap: &ProcSnapshot) -> environment::DepStates {
    std::thread::scope(|s| {
        let handles: Vec<_> = state
            .registry
            .iter()
            .map(|d| {
                let remote = remote_of(state, d);
                s.spawn(move || {
                    let st = to_status(d, state.procs.is_running(&d.id), snap, remote.as_ref());
                    (d.id.clone(), st, d.ports.clone(), remote)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                let (id, st, ports, remote) = h.join().expect("服务状态探测线程 panic");
                (
                    id,
                    environment::DepState {
                        name: st.name,
                        running: st.running,
                        healthy: st.healthy,
                        ports,
                        remote,
                    },
                )
            })
            .collect()
    })
}

/// 只构建环境检查所需服务子集的状态视图：自身 + 直接 dependsOn。
///
/// 单服务刷新不需要全量扫描——此前 env_check 为图省事构建全部服务的
/// statuses()，Windows 上等于为一个服务付一轮全量 TCP 探测的代价。
fn dep_states_for(
    state: &AppState,
    snap: &ProcSnapshot,
    ids: &[String],
) -> environment::DepStates {
    state
        .registry
        .iter()
        .filter(|d| ids.contains(&d.id))
        .map(|d| {
            let remote = remote_of(state, d);
            let st = to_status(d, state.procs.is_running(&d.id), snap, remote.as_ref());
            let dep = environment::DepState {
                name: st.name,
                running: st.running,
                healthy: st.healthy,
                ports: d.ports.clone(),
                remote,
            };
            (d.id.clone(), dep)
        })
        .collect()
}

/// 检查单个服务的运行环境（运行时依赖 + requires 声明 + dependsOn 依赖）。
pub async fn env_check(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<environment::ServiceEnvReport>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?.clone();
    let st = state.clone();
    let report = blocking(move || {
        let snap = ProcSnapshot::capture();
        // 环境检查只消费「自身 + 直接依赖」的状态（check_service_env_with 的
        // 全部读取点），无需全量 statuses 扫描
        let mut ids = def.depends_on.clone();
        ids.push(def.id.clone());
        let deps = dep_states_for(&st, &snap, &ids);
        environment::check_service_env_with(&def, &deps, None)
    })
    .await;
    // 单服务刷新结果同步进全量缓存对应项：「刷新此服务」后，缓存未过期的
    // 全量视图立即与最新探测一致，不会出现单服务/全量两份数据打架。
    if let Some((_, reports)) = state.env_cache.lock().unwrap().as_mut() {
        if let Some(slot) = reports.iter_mut().find(|r| r.id == report.id) {
            *slot = report.clone();
        }
    }
    Ok(Json(report))
}

// ===== 云端服务外部配置（config/application-local.yaml）运维 =====

/// 云端服务部署目录：设置里显式指定的 backendInstallDir 优先，否则用注册表内置值。
/// 设置锁在取值后立即释放，避免跨 await 持锁。
fn backend_install_dir(state: &AppState, def: &ServiceDef) -> std::path::PathBuf {
    let override_dir = state.app_settings.lock().unwrap().backend_install_dir.clone();
    if override_dir.is_empty() {
        std::path::PathBuf::from(&def.install_dir)
    } else {
        std::path::PathBuf::from(override_dir)
    }
}

/// 读取服务配置（dfy-backend → external_config，nginx → nginx_config）。
pub async fn external_config_get(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    if def.id == "nginx" {
        let install_dir = def.install_dir.clone();
        let cfg = blocking(move || crate::ops::nginx_config::read(std::path::Path::new(&install_dir))).await;
        let path = crate::ops::nginx_config::config_path(std::path::Path::new(&def.install_dir));
        return Ok(Json(match cfg {
            Ok(Some(c)) => serde_json::json!({
                "ok": true,
                "exists": true,
                "configPath": path.display().to_string(),
                "config": c,
            }),
            Ok(None) => serde_json::json!({
                "ok": true,
                "exists": false,
                "configPath": path.display().to_string(),
                "hint": "nginx.conf 不存在：请先通过资源目录安装 Nginx（nginx zip）。",
            }),
            Err(e) => {
                // 读失败（权限/被占用/编码）：前端只见「读取失败」，这里留下根因
                crate::debug_log::entry(
                    crate::debug_log::APP_SCOPE,
                    "WARN",
                    "nginx_config",
                    "read_failed",
                    &format!("读取 {} 失败：{e}", path.display()),
                );
                serde_json::json!({ "ok": false, "error": e })
            }
        }));
    }
    // edge-conductor：config.toml 字段级编辑（换 IP/端口的主要运维入口）
    if def.id == "edge-conductor" {
        let install_dir = def.install_dir.clone();
        let cfg = blocking(move || crate::ops::edge_config::read(std::path::Path::new(&install_dir))).await;
        let path = crate::ops::edge_config::config_path(std::path::Path::new(&def.install_dir));
        return Ok(Json(match cfg {
            Ok(Some(c)) => serde_json::json!({
                "ok": true,
                "exists": true,
                "configPath": path.display().to_string(),
                "config": c,
            }),
            Ok(None) => serde_json::json!({
                "ok": true,
                "exists": false,
                "configPath": path.display().to_string(),
                "hint": "config.toml 不存在：请先通过资源目录安装边缘计算（安装包或 edge-conductor/ 目录）。",
            }),
            Err(e) => {
                crate::debug_log::entry(
                    crate::debug_log::APP_SCOPE,
                    "WARN",
                    "edge_config",
                    "read_failed",
                    &format!("读取 {} 失败：{e}", path.display()),
                );
                serde_json::json!({ "ok": false, "error": e })
            }
        }));
    }
    let install_dir = backend_install_dir(&state, def);
    let path = crate::ops::external_config::config_path(&install_dir);
    let cfg = blocking(move || crate::ops::external_config::read(&install_dir)).await;
    match cfg {
        Ok(Some(c)) => Ok(Json(serde_json::json!({
            "ok": true,
            "exists": true,
            "configPath": path.display().to_string(),
            "config": c,
        }))),
        Ok(None) => Ok(Json(serde_json::json!({
            "ok": true,
            "exists": false,
            "configPath": path.display().to_string(),
            "hint": "外部配置不存在：服务使用 jar 内置默认配置。在部署目录创建 config/application-local.yaml 即可覆盖。",
        }))),
        Err(e) => {
            // 读失败（目录不存在但预期有/权限）：前端只见「读取失败」，这里留根因
            crate::debug_log::entry(
                crate::debug_log::APP_SCOPE,
                "WARN",
                "external_config",
                "read_failed",
                &format!("读取 {} 失败：{e}", path.display()),
            );
            Ok(Json(serde_json::json!({ "ok": false, "error": e })))
        }
    }
}

/// 更新服务配置（dfy-backend → external_config，nginx → nginx_config）。
pub async fn external_config_update(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    if def.id == "nginx" {
        let cfg: crate::ops::nginx_config::NginxConfig = match serde_json::from_value(body) {
            Ok(c) => c,
            Err(e) => {
                // 前端表单正常不会发出坏 payload；出现即说明前后端结构失配，留根因
                crate::debug_log::entry(
                    crate::debug_log::APP_SCOPE,
                    "WARN",
                    "nginx_config",
                    "parse_failed",
                    &format!("nginx 配置保存请求解析失败：{e}"),
                );
                return Ok(Json(serde_json::json!({ "ok": false, "error": format!("配置格式错误：{e}") })));
            }
        };
        // 审计摘要：写闭包会消费 cfg，先提取关键字段（均为转发拓扑，无敏感信息）
        let summary = format!(
            "server_name={}, listen={}, 反代 {}:{}",
            cfg.server_name, cfg.listen_port, cfg.backend_host, cfg.backend_port
        );
        let install_dir = def.install_dir.clone();
        let written = blocking(move || crate::ops::nginx_config::write(std::path::Path::new(&install_dir), &cfg)).await;
        // 写盘成功后自动热加载（nginx -s reload）：运行中立即生效，无需用户
        // 手动去点重载——这就是「改了配置立刻看到效果」的闭环。nginx 没在跑时
        // reload 必然失败，属预期，hint 里说明配置已在下次启动时生效。
        let mut reload_note = String::new();
        if let Ok(p) = &written {
            let exe = std::path::Path::new(&def.install_dir).join("nginx.exe");
            let prefix = def.install_dir.clone();
            let reload = blocking(move || {
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    // 隐藏控制台窗口：不隐藏的话每次保存配置都会弹黑色 cmd 框
                    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                    std::process::Command::new(&exe)
                        .args(["-s", "reload", "-p", prefix.as_str()])
                        .creation_flags(CREATE_NO_WINDOW)
                        .output()
                }
                #[cfg(not(target_os = "windows"))]
                {
                    std::process::Command::new(&exe)
                        .args(["-s", "reload", "-p", prefix.as_str()])
                        .output()
                }
            })
            .await;
            match reload {
                Ok(out) if out.status.success() => {
                    reload_note = "已自动热加载（nginx -s reload），配置立即生效。".into();
                }
                Ok(out) => {
                    let err = String::from_utf8_lossy(&out.stderr);
                    reload_note = format!(
                        "nginx 未在运行或热加载失败（{}），配置已写入，下次启动时生效。",
                        err.trim().chars().take(160).collect::<String>()
                    );
                }
                Err(e) => {
                    reload_note = format!(
                        "配置已写入；热加载调用失败（{e}），nginx 下次启动时生效。"
                    );
                }
            }
            crate::debug_log::entry(
                crate::ops::process::PROC_SCOPE,
                "INFO",
                "nginx_config",
                "write_and_reload",
                &format!("path={} reload_note={reload_note}", p.display()),
            );
            // 配置修改事件：改了哪些转发字段 + 生效方式（热加载/待重启），审计可回溯
            crate::ops_events::record(
                "user",
                "nginx",
                "config_update",
                true,
                &format!("{summary}；{reload_note}"),
            );
        }
        return Ok(Json(match written {
            Ok(p) => serde_json::json!({
                "ok": true,
                "configPath": p.display().to_string(),
                "hint": format!("已写入 nginx.conf。{reload_note}"),
            }),
            Err(e) => {
                crate::debug_log::entry(
                    crate::debug_log::APP_SCOPE,
                    "ERROR",
                    "nginx_config",
                    "write_failed",
                    &format!("写入 nginx.conf 失败（{summary}）：{e}"),
                );
                crate::ops_events::record("user", "nginx", "config_update", false, &format!("{summary}：{e}"));
                serde_json::json!({ "ok": false, "error": e })
            }
        }));
    }
    // edge-conductor：config.toml 保存（校验 + TOML 语法兜底 + 备份 + 原子写回）
    if def.id == "edge-conductor" {
        let cfg: crate::ops::edge_config::EdgeConfig = match serde_json::from_value(body) {
            Ok(c) => c,
            Err(e) => {
                crate::debug_log::entry(
                    crate::debug_log::APP_SCOPE,
                    "WARN",
                    "edge_config",
                    "parse_failed",
                    &format!("edge-conductor 配置保存请求解析失败：{e}"),
                );
                return Ok(Json(serde_json::json!({ "ok": false, "error": format!("配置格式错误：{e}") })));
            }
        };
        // 审计摘要：端口与地址拓扑（无凭据字段）
        let summary = format!(
            "http={}:{} ws={}:{} upstream={} mqtt={}:{}",
            cfg.http_bind, cfg.http_port, cfg.ws_bind, cfg.ws_port,
            cfg.upstream_server_url, cfg.mqtt_broker_url, cfg.mqtt_broker_port
        );
        let install_dir = def.install_dir.clone();
        let saved = blocking(move || {
            crate::ops::edge_config::save(std::path::Path::new(&install_dir), &cfg)
        })
        .await;
        return Ok(Json(match saved {
            Ok(()) => {
                crate::ops_events::record(
                    "user",
                    "edge-conductor",
                    "config_update",
                    true,
                    &summary,
                );
                serde_json::json!({
                    "ok": true,
                    "hint": "已保存 config.toml。重启边缘计算后生效（服务总览 → 启动）。",
                })
            }
            Err(e) => {
                crate::debug_log::entry(
                    crate::debug_log::APP_SCOPE,
                    "ERROR",
                    "edge_config",
                    "write_failed",
                    &format!("写入 config.toml 失败（{summary}）：{e}"),
                );
                crate::ops_events::record(
                    "user",
                    "edge-conductor",
                    "config_update",
                    false,
                    &format!("{summary}：{e}"),
                );
                serde_json::json!({ "ok": false, "error": e })
            }
        }));
    }
    let cfg: crate::ops::external_config::ExternalConfig = match serde_json::from_value(body) {
        Ok(c) => c,
        Err(e) => {
            crate::debug_log::entry(
                crate::debug_log::APP_SCOPE,
                "WARN",
                "external_config",
                "parse_failed",
                &format!("dfy-backend 外部配置保存请求解析失败：{e}"),
            );
            return Ok(Json(serde_json::json!({ "ok": false, "error": format!("配置格式错误：{e}") })));
        }
    };
    // 审计摘要：只取非敏感字段——连接串/密码一律不入日志（application-local.yaml
    // 里有 MySQL/Redis/VerneMQ 明文凭据，审计文件可能被截图传阅）
    let summary = format!("serverPort={}", cfg.server_port);
    let install_dir = backend_install_dir(&state, def);
    let written = blocking(move || crate::ops::external_config::write(&install_dir, &cfg)).await;
    match written {
        Ok(p) => {
            crate::debug_log::entry(
                crate::debug_log::APP_SCOPE,
                "INFO",
                "external_config",
                "write",
                &format!("dfy-backend 外部配置已写入 {}（{summary}）", p.display()),
            );
            crate::ops_events::record(
                "user",
                "dfy-backend",
                "config_update",
                true,
                &format!("{summary}，已写入 {}，重启服务后生效", p.display()),
            );
            Ok(Json(serde_json::json!({
                "ok": true,
                "configPath": p.display().to_string(),
                "hint": "已写入。重启服务后生效。",
            })))
        }
        Err(e) => {
            crate::debug_log::entry(
                crate::debug_log::APP_SCOPE,
                "ERROR",
                "external_config",
                "write_failed",
                &format!("dfy-backend 外部配置写入失败（{summary}）：{e}"),
            );
            crate::ops_events::record("user", "dfy-backend", "config_update", false, &format!("{summary}：{e}"));
            Ok(Json(serde_json::json!({ "ok": false, "error": e })))
        }
    }
}

/// `open-config-dir` 查询参数：缺省打开配置目录；`dir=logs` 打开 nginx 日志目录；
/// `path=<日志文件>` 打开该文件所在目录（监控中心用，任意服务通用）。
#[derive(Debug, Deserialize)]
pub struct OpenDirQuery {
    dir: Option<String>,
    path: Option<String>,
}

/// 在系统文件管理器中打开服务配置/日志目录（只读便利操作）。
/// 缺省：nginx → conf/（nginx.conf）；dfy-backend → config/（application-local.yaml）。
/// `?dir=logs`：nginx → logs/（access.log / error.log，未创建时顺带建目录）。
/// 纯浏览不入 ops_events 审计，debug_log 留痕即可。
pub async fn open_config_dir(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<OpenDirQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?;
    let want_logs = q.dir.as_deref() == Some("logs");
    let install = std::path::Path::new(&def.install_dir);
    // 监控中心「打开目录」：按选中日志文件的路径打开其所在目录（任意服务通用）。
    // 此前只有 nginx 支持 ?dir=logs，其余服务一律返回「暂不支持」且前端静默吞错，
    // 表现就是点按钮没反应。走日志白名单校验，防任意路径探测。
    if let Some(p) = q.path.as_deref() {
        if !log_path_allowed(def, p) {
            return Ok(Json(serde_json::json!({
                "ok": false,
                "error": "该文件不在此服务的日志声明范围内",
            })));
        }
        let file = std::path::Path::new(p);
        let dir = match file.parent() {
            Some(d) if d.exists() => d.to_path_buf(),
            Some(d) => {
                return Ok(Json(serde_json::json!({
                    "ok": false,
                    "error": format!("目录不存在：{}", d.display()),
                })));
            }
            None => {
                return Ok(Json(serde_json::json!({ "ok": false, "error": "路径异常：无父目录" })));
            }
        };
        // 定位到文件本身：资源管理器打开所在目录并高亮该日志（2026-11-05 用户要求）
        return match open_in_file_manager_select(file) {
            Ok(_) => {
                crate::debug_log::entry(
                    crate::debug_log::APP_SCOPE,
                    "INFO",
                    "open_config_dir",
                    "open_select",
                    &format!("open-log-file-dir target={} file={} dir={}", id, file.display(), dir.display()),
                );
                Ok(Json(serde_json::json!({ "ok": true, "dir": dir.display().to_string() })))
            }
            Err(e) => {
                crate::debug_log::entry(
                    crate::debug_log::APP_SCOPE,
                    "WARN",
                    "open_config_dir",
                    "open_failed",
                    &format!("定位日志文件 {} 失败：{e}", file.display()),
                );
                Ok(Json(serde_json::json!({
                    "ok": false,
                    "error": format!("打开失败：{e}"),
                    "dir": dir.display().to_string(),
                })))
            }
        };
    }
    let path = if def.id == "nginx" {
        if want_logs {
            install.join("logs").join("access.log")
        } else {
            crate::ops::nginx_config::config_path(install)
        }
    } else {
        if want_logs {
            return Ok(Json(serde_json::json!({
                "ok": false,
                "error": "该服务暂不支持打开日志目录",
            })));
        }
        crate::ops::external_config::config_path(&backend_install_dir(&state, def))
    };
    let dir = match path.parent() {
        Some(d) => d.to_path_buf(),
        None => {
            return Ok(Json(serde_json::json!({ "ok": false, "error": "路径异常：无父目录" })));
        }
    };
    if !dir.exists() {
        // 日志目录没生成过（nginx 装完未启动过）顺带创建，避免用户第一次点就报错；
        // 配置目录不存在说明服务未安装，如实报错
        if want_logs {
            let _ = std::fs::create_dir_all(&dir);
        }
        if !dir.exists() {
            return Ok(Json(serde_json::json!({
                "ok": false,
                "error": format!("目录不存在：{}（服务可能未安装）", dir.display()),
            })));
        }
    }
    match open_in_file_manager(&dir) {
        Ok(_) => {
            crate::debug_log::entry(
                crate::debug_log::APP_SCOPE,
                "INFO",
                "open_config_dir",
                "open",
                &format!("open-config-dir target={} dir={}", id, dir.display()),
            );
            Ok(Json(serde_json::json!({ "ok": true, "dir": dir.display().to_string() })))
        }
        Err(e) => {
            crate::debug_log::entry(
                crate::debug_log::APP_SCOPE,
                "WARN",
                "open_config_dir",
                "open_failed",
                &format!("打开目录 {} 失败：{e}", dir.display()),
            );
            Ok(Json(serde_json::json!({
                "ok": false,
                "error": format!("打开失败：{e}"),
                "dir": dir.display().to_string(),
            })))
        }
    }
}

/// 一键打开平台页面：读取 nginx 实际监听端口生成 URL，用默认浏览器打开。
/// 安装完成后闭环的最后一厘米——不再让用户自己开浏览器敲 localhost。
/// URL 以 nginx.conf 的 listen 为准（用户改过端口也能打开对的地址）。
pub async fn platform_open(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    // 读 nginx.conf 的 listen（未装/读取失败时兜底 80）
    let port = match find_def(&state, "nginx") {
        Some(def) => {
            let dir = std::path::PathBuf::from(&def.install_dir);
            match crate::ops::nginx_config::read(&dir) {
                Ok(Some(c)) if !c.listen_port.trim().is_empty() => c.listen_port.trim().to_string(),
                _ => "80".to_string(),
            }
        }
        None => "80".to_string(),
    };
    let url = format!("http://localhost:{port}/");
    // 打开前探测 Nginx 运行态：浏览器能开但页面 502（Nginx 没跑/后端没跑）时
    // 用户会以为按钮坏了——如实在响应里预警，前端展示提醒
    let nginx_running = tokio::task::spawn_blocking(|| {
        crate::api::handlers_repair::image_running("nginx.exe")
    })
    .await
    .unwrap_or(false);
    let url_for_cmd = url.clone();
    let result = tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            std::process::Command::new("cmd")
                .args(["/C", "start", "", &url_for_cmd])
                .creation_flags(0x0800_0000)
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::process::Command::new("open")
                .arg(&url_for_cmd)
                .spawn()
                .map(|_| ())
                .map_err(|e| e.to_string())
        }
    })
    .await;
    match result {
        Ok(Ok(())) => {
            crate::debug_log::entry(
                crate::debug_log::APP_SCOPE,
                "INFO",
                "platform_open",
                "open",
                &format!("打开平台页面 {url}"),
            );
            crate::ops_events::record("user", "nginx", "platform_open", true, &format!("浏览器打开 {url}"));
            Json(serde_json::json!({
                "ok": true,
                "url": url,
                "nginxRunning": nginx_running,
                "warning": if nginx_running { None } else {
                    Some("Nginx 未运行，页面可能无法访问——请先到「修复中心 → 安装维护」启动 Nginx".to_string())
                },
            }))
        }
        Ok(Err(m)) => {
            crate::debug_log::entry(
                crate::debug_log::APP_SCOPE,
                "WARN",
                "platform_open",
                "open_failed",
                &format!("打开 {url} 失败：{m}"),
            );
            Json(serde_json::json!({ "ok": false, "error": format!("打开浏览器失败：{m}"), "url": url }))
        }
        Err(j) => {
            crate::debug_log::entry(
                crate::debug_log::APP_SCOPE,
                "WARN",
                "platform_open",
                "open_failed",
                &format!("打开 {url} 任务异常：{j}"),
            );
            Json(serde_json::json!({ "ok": false, "error": format!("打开浏览器失败：{j}"), "url": url }))
        }
    }
}

/// 一键诊断快照：把「排查一次故障要看的所有状态」汇总成一段 JSON——
/// 版本、各服务安装/运行态、关键端口占用、最近 20 条运维事件。
/// 用户点「复制诊断」粘给维护方（或 AI），不用再手动翻 debug.log。
pub async fn diagnostics_snapshot(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    // 各服务判定：复用修复中心的探测口径（install_dir + 关键文件 + 运行态）
    let mut services = serde_json::Map::new();
    for id in ["mysql", "redis", "java", "nginx", "dfy-backend", "dfy-ui", "edge-conductor"] {
        let (installed, dir_ok, running) =
            crate::api::handlers_repair::probe_target(&state, id).await;
        services.insert(
            id.to_string(),
            serde_json::json!({ "installed": installed, "dirOk": dir_ok, "running": running }),
        );
    }
    // 关键端口占用（48085=后端，80=平台入口；占用者一并给出）
    let ports = tokio::task::spawn_blocking(|| {
        let b = crate::api::handlers_repair::port_holder(48085)
            .map(|(pid, n)| format!("{pid} {n}"));
        let n = crate::api::handlers_repair::port_holder(80).map(|(pid, n)| format!("{pid} {n}"));
        serde_json::json!({ "48085": b, "80": n })
    })
    .await
    .unwrap_or(serde_json::json!({}));
    // 最近 20 条运维事件（时间轴快照）
    let events = crate::ops_events::recent(20)
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "ts": e.ts, "actor": e.actor, "target": e.target,
                "action": e.action, "ok": e.ok, "detail": e.detail,
            })
        })
        .collect::<Vec<_>>();
    // debug.log 尾部 30 行：最近的日志现场直接进快照，维护方少一步要文件
    let debug_log_tail = tokio::task::spawn_blocking(|| crate::debug_log::tail_lines(30))
        .await
        .unwrap_or_default();
    let snapshot = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "generatedAt": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        "services": services,
        "ports": ports,
        "recentEvents": events,
        "debugLogTail": debug_log_tail,
    });
    crate::debug_log::entry(
        crate::debug_log::APP_SCOPE,
        "INFO",
        "diagnostics",
        "snapshot",
        &format!("生成诊断快照（{} 字节）", snapshot.to_string().len()),
    );
    Json(serde_json::json!({ "ok": true, "snapshot": snapshot }))
}

/// 环境检查全量结果的缓存有效期：全量探测含多次子进程调用（java/-version 等），开销大。
const ENV_CACHE_TTL: Duration = Duration::from_secs(60);

/// GET /api/env 查询参数：`force=1` 跳过缓存强制重探。
///
/// 手动「重新检查」必须走 force——用户刚修完环境（装了 JDK / 起了中间件）
/// 点复检，若被 60s 缓存吞掉，看到的是修复前的旧结论，属于准确度 bug。
#[derive(Debug, Deserialize)]
pub struct EnvCheckQuery {
    #[serde(default)]
    force: bool,
}

/// 检查全部服务的运行环境（60s TTL 缓存 + single-flight + 并行探测）。
///
/// - 缓存：自动检测（bootstrap/轮询兜底）命中 TTL 内缓存，避免重复子进程探测；
/// - single-flight：缓存过期瞬间的并发请求只放一个进去重探（tokio 异步锁），
///   其余等锁后命中新缓存——否则 N 个并发请求就是 N 轮全量子进程探测（击穿风暴）；
/// - 并行探测：状态视图（快照 + 并行 to_status）与环境检查（thread::scope +
///   ProbeCache）并行执行，java -version 等重复命令收敛为一次子进程调用。
pub async fn env_check_all(
    State(state): State<Arc<AppState>>,
    Query(q): Query<EnvCheckQuery>,
) -> Json<Vec<environment::ServiceEnvReport>> {
    if !q.force {
        if let Some((at, reports)) = state.env_cache.lock().unwrap().as_ref() {
            if at.elapsed() < ENV_CACHE_TTL {
                return Json(reports.clone());
            }
        }
    }
    // 异步锁等待期间不占用 worker 线程；拿到锁后双检，防止等锁期间已被其他请求重建
    // （force 请求跳过双检：等锁期间即便有重建，本请求的语义仍是「此刻强制重探」）
    let _guard = state.env_rebuild.lock().await;
    if !q.force {
        if let Some((at, reports)) = state.env_cache.lock().unwrap().as_ref() {
            if at.elapsed() < ENV_CACHE_TTL {
                return Json(reports.clone());
            }
        }
    }
    let st = state.clone();
    let reports = blocking(move || {
        let snap = ProcSnapshot::capture();
        let deps = dep_states(&st, &snap);
        let cache = environment::ProbeCache::default();
        std::thread::scope(|s| {
            // move 闭包捕获引用本身（Copy），避免把 deps/cache 整体 move 进闭包
            let deps = &deps;
            let cache = &cache;
            let handles: Vec<_> = st
                .registry
                .iter()
                .map(|d| {
                    s.spawn(move || {
                        environment::check_service_env_with(d, deps, Some(cache))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("环境检查线程 panic"))
                .collect::<Vec<_>>()
        })
    })
    .await;
    *state.env_cache.lock().unwrap() = Some((Instant::now(), reports.clone()));
    Json(reports)
}

/// 单服务组合刷新：一次进程快照同时产出状态与环境报告。
///
/// 前端「启停后局部刷新 / 刷新此服务」原先要打 status + env 两个请求，
/// 各采一份进程快照（Windows 上 tasklist+netstat 各跑两轮）；合并后一份
/// 快照两份产出，单服务刷新的等待时间近乎减半。
pub async fn service_refresh(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let def = find_def(&state, &id).ok_or(StatusCode::NOT_FOUND)?.clone();
    let st = state.clone();
    let (status, report) = blocking(move || {
        let snap = ProcSnapshot::capture();
        let remote = remote_of(&st, &def);
        let status = to_status(&def, st.procs.is_running(&id), &snap, remote.as_ref());
        let mut ids = def.depends_on.clone();
        ids.push(def.id.clone());
        let deps = dep_states_for(&st, &snap, &ids);
        let report = environment::check_service_env_with(&def, &deps, None);
        (status, report)
    })
    .await;
    // 同步进全量缓存对应项（与 env_check 相同口径，保持单服务/全量视图一致）
    if let Some((_, reports)) = state.env_cache.lock().unwrap().as_mut() {
        if let Some(slot) = reports.iter_mut().find(|r| r.id == report.id) {
            *slot = report.clone();
        }
    }
    Ok(Json(serde_json::json!({ "status": status, "env": report })))
}

// ===== 应用设置（「设置」页，与 AI 助手配置分离） =====

#[derive(Deserialize)]
pub struct AppSettingsInput {
    #[serde(rename = "statusRefreshSecs")]
    status_refresh_secs: u64,
    #[serde(rename = "envRefreshSecs")]
    env_refresh_secs: u64,
    #[serde(rename = "resourceDir", default)]
    resource_dir: String,
}

/// 读取应用设置。
pub async fn get_app_settings(
    State(state): State<Arc<AppState>>,
) -> Json<crate::ops::appsettings::AppSettings> {
    Json(state.app_settings.lock().unwrap().clone())
}

/// 保存应用设置到 config_dir/settings.json 并热更新内存（轮询间隔立即生效）。
pub async fn save_app_settings(
    State(state): State<Arc<AppState>>,
    Json(body): Json<AppSettingsInput>,
) -> Json<serde_json::Value> {
    let mut s = state.app_settings.lock().unwrap().clone();
    s.status_refresh_secs = body.status_refresh_secs;
    s.env_refresh_secs = body.env_refresh_secs;
    s.resource_dir = body.resource_dir;
    s.sanitize();
    let config_dir = state.config_dir.clone();
    let result = blocking(move || match s.save(&config_dir) {
        Ok(_) => Ok(s),
        Err(e) => Err(e),
    })
    .await;
    match result {
        Ok(s) => {
            *state.app_settings.lock().unwrap() = s.clone();
            Json(serde_json::json!({
                "ok": true,
                "statusRefreshSecs": s.status_refresh_secs,
                "envRefreshSecs": s.env_refresh_secs,
                "resourceDir": s.resource_dir,
            }))
        }
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}
