//! 环境修复中心：本地资源目录扫描、通用修复任务、AI 结构化修复方案、白名单代执行。
//!
//! 修复来源降级链（与 ops/resource.rs 的模块文档一致）：
//! 1. 本地资源目录匹配包 → UAC 提权静默安装（「一键修复」）；
//! 2. 在线下载（仅 java 有 Adoptium 链路；有网时）；
//! 3. AI 结构化方案（steps 中白名单内的命令可确认后代执行，其余复制手动执行）。

use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::api::server::AppState;
use crate::ops::ai;
use crate::ops::resource;

/// 与 handlers.rs 的 blocking 语义一致：同步阻塞段移交 tokio blocking 池。
async fn blocking<T, F>(f: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .expect("blocking task panicked")
}

// ===== 资源目录扫描 =====

#[derive(Debug, Deserialize)]
pub struct ResourcesQuery {
    /// 传入时扫描该目录（设置页对未保存草稿的即时预览）；缺省扫已配置目录
    #[serde(default)]
    pub dir: Option<String>,
}

/// GET /api/env/resources —— 扫描「设置 → 本地资源目录」，列出识别出的安装包。
/// 设置页预览与环境修复中心共用（目录内容用户随时在改，每次实时扫，不做缓存）。
pub async fn resources(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(q): axum::extract::Query<ResourcesQuery>,
) -> Json<resource::ResourceScan> {
    let dir = match q.dir {
        Some(d) if !d.trim().is_empty() => d,
        _ => state.app_settings.lock().unwrap().resource_dir.clone(),
    };
    let scan = blocking(move || resource::scan(&dir)).await;
    Json(scan)
}

// ===== 通用修复任务 =====

#[derive(Debug, Deserialize)]
pub struct GenericRepairInput {
    /// 检查项 id 或目标软件名（dep-mysql / service-vernemq / java / mysql …）
    pub target: String,
}

/// POST /api/env/repair/generic —— 通用修复决策链：
/// 1. 资源目录有该目标的 ZIP 免安装版（mysql/redis/java）→ 解压 → 配置 →（注册服务）；
///    java 免安装版 = 写用户级 JAVA_HOME + 平台绝对路径指向，无 MSI/注册表/UAC；
///    MySQL 官方 msi 是 Installer 束，静默执行装不上 Server，故 zip 优先；
/// 2. 有 MSI（java/vernemq/memurai 等）→ msiexec 静默安装；
/// 3. 无包且 target=java → 在线下载 Adoptium 免安装版 zip（同 1 的解压管线）；
/// 4. 无包且其他目标 → 提示配置资源目录或使用 AI 方案。
/// 任务状态复用 /api/env/repair/status（与 java 修复同一全局单例，前端轮询逻辑不变）。
pub async fn start_generic_repair(
    State(state): State<Arc<AppState>>,
    Json(input): Json<GenericRepairInput>,
) -> Json<serde_json::Value> {
    let Some(target) = resource::repair_target(&input.target) else {
        return Json(serde_json::json!({
            "ok": false,
            "error": "该检查项不支持自动安装修复，请使用 AI 方案或按修复建议手动处理"
        }));
    };
    let label = resource::target_label(target);

    let dir = state.app_settings.lock().unwrap().resource_dir.clone();
    let (zip, msi) = if dir.is_empty() {
        (None, None)
    } else {
        blocking(move || (resource::find_zip(&dir, target), resource::find_msi(&dir, target))).await
    };

    // mysql/redis/java 支持免安装版管线；java 的安装目录固定平台托管位置
    let zip_capable = matches!(target, "mysql" | "redis" | "java");
    let started = if zip_capable && zip.is_some() {
        let install_dir = if target == "java" {
            crate::ops::env_repair::JDK_MANAGED_HOME.to_string()
        } else {
            state
                .registry
                .iter()
                .find(|d| d.id == target)
                .map(|d| d.install_dir.clone())
                .unwrap_or_else(|| format!("C:\\services\\{target}"))
        };
        crate::ops::env_repair::start_zip_install(
            target,
            zip.clone().unwrap().path,
            install_dir,
            state.env_cache.clone(),
            state.repair.clone(),
        )
    } else if let Some(f) = msi.clone() {
        crate::ops::env_repair::start_resource_install(
            target,
            f.path,
            state.env_cache.clone(),
            state.repair.clone(),
        )
    } else if target == "java" && zip.is_none() && msi.is_none() {
        // 无本地包的 java 在线兜底（免安装版 zip；expected 按注册表最低声明 8）
        crate::ops::env_repair::start_jdk_install(
            crate::ops::env_repair::JDK_DEFAULT_MAJOR,
            state.env_cache.clone(),
            state.repair.clone(),
        )
    } else {
        Err(format!(
            "本地资源目录中没有 {label} 的可安装包。请将 MSI 安装包（或 MySQL/Redis/Java 的 ZIP 免安装版）放入「设置 → 本地资源目录」后重试，或使用 AI 方案"
        ))
    };

    let source = if zip.is_some() && zip_capable {
        Some("zip")
    } else if msi.is_some() {
        Some("msi")
    } else {
        None
    };
    match started {
        Ok(()) => Json(serde_json::json!({
            "ok": true,
            "action": "install",
            "message": match source {
                Some("zip") if target == "java" => format!("开始离线安装 {label}（免安装版：解压 → 写 JAVA_HOME → 版本验证，无需管理员权限），进度见下方任务状态"),
                Some("zip") => format!("开始离线安装 {label}（免安装版：解压 → 初始化 → 注册服务），全程只需一次管理员确认，进度见下方任务状态"),
                Some("msi") => format!("开始离线安装 {label}（{}）→ 管理员确认 → 验证，进度见下方任务状态", msi.as_ref().map(|f| f.file_name.as_str()).unwrap_or_default()),
                _ => format!("资源目录无 {label} 安装包，开始在线安装免安装版（下载 → 解压 → 验证）"),
            }
        })),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

// ===== 卸载 / 维护 =====

/// 平台内卸载覆盖：mysql/redis（zip 管线，服务+目录全由平台掌控）+ java
/// （MSI 注册表条目静默卸载）。其余目标建议 AI 方案或控制面板手动卸载。
pub const UNINSTALLABLE: [&str; 3] = ["mysql", "redis", "java"];

fn registry_install_dir(state: &AppState, target: &str) -> String {
    state
        .registry
        .iter()
        .find(|d| d.id == target)
        .map(|d| d.install_dir.clone())
        .unwrap_or_else(|| format!("C:\\services\\{target}"))
}

/// 探测一个目标的安装痕迹（服务存在 + 目录存在；java = MSI 条目或平台免安装
/// 目录；mysql 的服务检测叠加 MSI 产品条目——外部 MySQL Installer 装的也算已装）。
/// 同时返回服务运行态（运维面板展示与启停按钮状态用；java 无服务恒 false）。
async fn probe_target(state: &AppState, target: &str) -> (bool, bool, bool) {
    if target == "java" {
        let installed = blocking(|| {
            !crate::ops::env_repair::java_msi_entries().is_empty()
                || crate::ops::env_repair::managed_java_home().is_some()
        })
        .await;
        return (installed, false, false);
    }
    let Some((svc, _)) = crate::ops::env_repair::service_spec(target) else {
        return (false, false, false);
    };
    let install_dir = registry_install_dir(state, target);
    let msi = target == "mysql";
    let (svc_exists, dir_exists, msi_installed, running) = blocking(move || {
        let exists = crate::ops::env_repair::service_exists(svc);
        let running = exists && crate::ops::env_repair::service_running(svc);
        (
            exists,
            std::path::Path::new(&install_dir).is_dir(),
            msi && !crate::ops::env_repair::mysql_msi_entries().is_empty(),
            running,
        )
    })
    .await;
    (svc_exists || msi_installed, dir_exists, running)
}

/// GET /api/env/repair/uninstall/status —— 可卸载组件的安装痕迹探测，
/// 修复中心「已安装组件管理」与 AI 方案上下文共用（每次实时探，sc query 很快）。
pub async fn uninstall_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let mut targets = Vec::new();
    for t in UNINSTALLABLE {
        let (service_installed, dir_exists, service_running) = probe_target(&state, t).await;
        // java 补充实测信息：平台托管主目录 + 实跑 java -version 的版本号
        // （MSI 装的从卸载条目名提取主版本）+ 注册表核实的 JAVA_HOME 配置态，
        // 让「装好了没、什么版本、环境变量配了没」在卡片上一眼可见
        let (java_dir, java_version, java_home_ok) = if t == "java" {
            blocking(|| {
                let dir = crate::ops::env_repair::managed_java_home();
                let ver = dir
                    .as_ref()
                    .and_then(|h| crate::ops::env_repair::probe_home_version(h))
                    .map(|(a, b, c)| format!("{a}.{b}.{c}"))
                    .or_else(crate::ops::env_repair::java_msi_version_label);
                let ok = crate::ops::env_repair::java_home_configured(dir.as_deref());
                (dir, ver, ok)
            })
            .await
        } else {
            (None, None, false)
        };
        targets.push(serde_json::json!({
            "target": t,
            "label": resource::target_label(t),
            // java：平台 zip 管线装的返回托管主目录（MSI 装的由 Windows Installer 管理，为空）
            "installDir": if t == "java" {
                java_dir.unwrap_or_default()
            } else {
                registry_install_dir(&state, t)
            },
            "version": java_version,
            "javaHomeConfigured": java_home_ok,
            "serviceInstalled": service_installed,
            "dirExists": dir_exists,
            "serviceRunning": service_running,
            "canUninstall": service_installed || dir_exists,
        }));
    }
    Json(serde_json::json!({ "targets": targets }))
}

#[derive(Deserialize)]
pub struct ServiceActionReq {
    pub target: String,
    /// "start" | "stop"
    pub action: String,
}

/// POST /api/env/repair/service —— 启动 / 停止平台注册的第三方服务
/// （MySQL80、Redis）。net start/stop 需要管理员权限，走 runas_wait 一次 UAC；
/// 幂等：已运行再 start、已停止再 stop 直接成功返回。
pub async fn service_action(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ServiceActionReq>,
) -> Json<serde_json::Value> {
    if !UNINSTALLABLE.contains(&req.target.as_str()) {
        return Json(serde_json::json!({ "ok": false, "error": format!("不支持的服务：{}", req.target) }));
    }
    let Some((svc, _)) = crate::ops::env_repair::service_spec(&req.target) else {
        return Json(serde_json::json!({ "ok": false, "error": format!("未找到 { } 的服务定义", req.target) }));
    };
    let verb = match req.action.as_str() {
        "start" => "start",
        "stop" => "stop",
        other => {
            return Json(serde_json::json!({ "ok": false, "error": format!("未知操作：{other}") }))
        }
    };
    let svc = svc.to_string();
    let svc_probe = svc.clone();
    let running_now = blocking(move || crate::ops::env_repair::service_running(&svc_probe)).await;
    if running_now == (verb == "start") {
        return Json(serde_json::json!({ "ok": true, "running": running_now, "message": "已是目标状态，无需操作" }));
    }
    let args = format!("{verb} {svc}");
    let label = format!("{} 服务{}", crate::ops::resource::target_label(&req.target), if verb == "start" { "启动" } else { "停止" });
    let res = blocking(move || crate::ops::env_repair::runas_wait("net", &args)).await;
    match res {
        Ok(()) => {
            let svc2 = svc.clone();
            let running = blocking(move || crate::ops::env_repair::service_running(&svc2)).await;
            // net stop 后 STATE 短暂过渡，以复核为准；失败给出明确原因
            if running == (verb == "start") {
                state.repair.log(format!("{label} 完成"));
                Json(serde_json::json!({ "ok": true, "running": running, "message": format!("{label}成功") }))
            } else {
                Json(serde_json::json!({ "ok": false, "error": format!("{label}后状态未达预期（当前{}），可在服务管理器查看详情", if running { "运行中" } else { "已停止" }) }))
            }
        }
        Err(e) => Json(serde_json::json!({ "ok": false, "error": format!("{label}失败：{e}") })),
    }
}

#[derive(Deserialize)]
pub struct MysqlImportReq {
    /// 本机 .sql 文件绝对路径（前端原生文件选择器取得）
    pub file: String,
}

/// POST /api/env/repair/mysql/import —— 导入 SQL 到本机 MySQL。
/// mysql 客户端从平台安装目录定位；文件内容经 stdin 直灌（无 shell 重定向，
/// 路径含空格/中文均安全）。初始化默认 root 空密码，与安装口径一致。
pub async fn mysql_import(
    State(state): State<Arc<AppState>>,
    Json(req): Json<MysqlImportReq>,
) -> Json<serde_json::Value> {
    let dir = registry_install_dir(&state, "mysql");
    let file = req.file;
    let file_for_log = file.clone();
    let res = blocking(move || {
        let client = format!("{dir}\\bin\\mysql.exe");
        let client = if std::path::Path::new(&client).is_file() {
            client
        } else {
            return Err(format!("未找到 mysql 客户端（{client}），请确认 MySQL 已由平台安装"));
        };
        if !std::path::Path::new(&file).is_file() {
            return Err(format!("SQL 文件不存在：{file}"));
        }
        let f = std::fs::File::open(&file).map_err(|e| format!("无法读取 SQL 文件：{e}"))?;
        let out = std::process::Command::new(&client)
            .args(["-h", "127.0.0.1", "-P", "3306", "-u", "root", "--default-character-set=utf8mb4"])
            .stdin(std::process::Stdio::from(f))
            .output()
            .map_err(|e| format!("无法启动 mysql 客户端：{e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            let err = String::from_utf8_lossy(&out.stderr);
            let tail: String = err
                .lines()
                .filter(|l| l.contains("ERROR") || !l.trim().is_empty())
                .rev()
                .take(3)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(" | ");
            Err(format!("导入失败（服务运行中且 root 密码为初始空密码时才可免密导入）：{tail}"))
        }
    })
    .await;
    match res {
        Ok(()) => {
            state.repair.log(format!("MySQL 导入完成：{}", file_for_log));
            Json(serde_json::json!({ "ok": true, "message": "导入完成" }))
        }
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

#[derive(Debug, Deserialize)]
pub struct UninstallInput {
    pub target: String,
    /// 同时删除安装目录（含数据库数据——不可恢复，前端弹窗默认不勾）
    #[serde(rename = "removeData", default)]
    pub remove_data: bool,
}

/// POST /api/env/repair/uninstall —— 卸载：mysql/redis 一次 UAC 完成停服务 +
/// 注销服务（removeData 时连安装目录一起删）；java 为 MSI 条目静默卸载
/// （removeData 不适用）。任务状态复用 /api/env/repair/status（同一全局单例）。
pub async fn uninstall(
    State(state): State<Arc<AppState>>,
    Json(input): Json<UninstallInput>,
) -> Json<serde_json::Value> {
    let Some(target) = resource::repair_target(&input.target) else {
        return Json(serde_json::json!({"ok": false, "error": "未知的目标组件"}));
    };
    if !UNINSTALLABLE.contains(&target) {
        return Json(serde_json::json!({"ok": false, "error": "该组件暂不支持平台内卸载（当前覆盖 MySQL/Redis/Java），请使用 AI 方案或控制面板手动卸载"}));
    }
    let install_dir = registry_install_dir(&state, target);
    let message = if target == "java" {
        "开始卸载 Java 运行时（枚举 MSI 条目 → 静默逐条卸载）→ 管理员确认 → 复核，进度见下方任务状态".to_string()
    } else {
        format!(
            "开始卸载 {}（{}数据目录）→ 管理员确认 → 复核，进度见下方任务状态",
            resource::target_label(target),
            if input.remove_data { "含" } else { "保留" }
        )
    };
    match crate::ops::env_repair::start_uninstall(
        target,
        input.remove_data,
        install_dir,
        state.config_dir.join("backups").to_string_lossy().to_string(),
        state.env_cache.clone(),
        state.repair.clone(),
    ) {
        Ok(()) => Json(serde_json::json!({"ok": true, "action": "uninstall", "message": message})),
        Err(e) => Json(serde_json::json!({"ok": false, "error": e})),
    }
}

// ===== AI 结构化修复方案 =====

#[derive(Debug, Deserialize)]
pub struct PlanCheckItem {
    pub id: String,
    pub name: String,
    pub status: String,
    pub expected: String,
    pub actual: String,
    #[serde(rename = "fixHint", default)]
    pub fix_hint: String,
    #[serde(default)]
    pub category: String,
}

#[derive(Debug, Deserialize)]
pub struct RepairPlanInput {
    #[serde(rename = "checkItems")]
    pub check_items: Vec<PlanCheckItem>,
    /// 上一轮方案文本（多轮追问时携带，供模型延续上下文修正方案）
    #[serde(rename = "previousPlan", default)]
    pub previous_plan: Option<String>,
    /// 用户追问内容（多轮对话）
    #[serde(rename = "followUp", default)]
    pub follow_up: Option<String>,
}

/// 方案里的一个步骤。command 在代执行白名单内时前端弹确认框后代执行，
/// 否则只提供「复制」——AI 生成的内容不可信，绝不静默执行。
#[derive(Debug, Clone, Serialize)]
pub struct PlanStep {
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(rename = "isExecutable", default)]
    pub is_executable: bool,
}

#[derive(Debug, Serialize)]
pub struct RepairPlan {
    pub ok: bool,
    /// "ai" = 模型返回合法 JSON；"text" = 模型内容非 JSON，原文兜底展示
    pub mode: String,
    pub summary: String,
    pub steps: Vec<PlanStep>,
    pub error: Option<String>,
}

fn build_plan_prompt(
    items: &[PlanCheckItem],
    files: &[resource::ResourceFile],
    installed: &[(String, bool)],
    previous_plan: Option<&str>,
    follow_up: Option<&str>,
) -> String {
    let mut p = String::from(
        "你是 Windows 服务器运维专家。以下是运维平台环境检查发现的问题，请给出修复方案。\n\n\
         [环境检查问题]\n",
    );
    for it in items {
        p.push_str(&format!(
            "- id={} 名称={} 分类={} 状态={} 期望=[{}] 实际=[{}] 平台建议=[{}]\n",
            it.id, it.name, it.category, it.status, it.expected, it.actual, it.fix_hint
        ));
    }
    if installed.is_empty() {
        p.push_str("\n[已安装组件] 探测失败（跳过）\n");
    } else {
        p.push_str("\n[已安装组件（Windows 服务级探测）]\n");
        for (label, on) in installed {
            p.push_str(&format!(
                "- {label}：{}\n",
                if *on { "已安装（有 Windows 服务）" } else { "未安装" }
            ));
        }
    }
    if let Some(prev) = previous_plan {
        p.push_str("\n[上一轮方案]\n");
        p.push_str(prev);
        p.push_str("\n（请在上一轮方案基础上，结合用户追问调整并输出完整的新方案 JSON）\n");
    }
    if let Some(fq) = follow_up {
        p.push_str("\n[用户追问]\n");
        p.push_str(fq);
        p.push_str("\n");
    }
    if files.is_empty() {
        p.push_str("\n[本地资源目录] 未配置或没有识别出的安装包。\n");
    } else {
        p.push_str("\n[本地资源目录可用的安装包]（优先使用本地包离线安装）\n");
        for f in files {
            let target = f.target.as_deref().unwrap_or("未识别");
            p.push_str(&format!(
                "- {}（{}，{} MB，识别为 {}）\n",
                f.file_name,
                f.path,
                f.size_bytes / 1024 / 1024,
                target
            ));
        }
    }
    p.push_str(
        "\n[代执行白名单] msiexec, winget, choco, sc, net, netsh, robocopy, xcopy, curl\n\
         平台可以代执行白名单内程序的【单条】命令（执行前用户会确认）。禁止管道、重定向、多条命令拼接（&& ; | 均不可用）。\n\n\
         要求：\n\
         1. 只回复 JSON，不要 markdown 代码块或其他文字；\n\
         2. 格式：{\"summary\": \"一句话方案概述\", \"steps\": [{\"title\": \"步骤名\", \"detail\": \"具体说明\", \"command\": \"可选，白名单内单条命令\", \"needsAdmin\": true|false}]}\n\
         3. 本地资源目录里有对应安装包时，优先给出 msiexec /i \"完整路径\" /quiet 形式的命令；\n\
         4. steps 不超过 6 步，按执行顺序排列。\n",
    );
    p
}

/// 纯文本兜底：模型没给 JSON 时原文透传（不丢信息）。
fn text_fallback(text: &str) -> RepairPlan {
    RepairPlan {
        ok: true,
        mode: "text".into(),
        summary: format!("AI 未返回结构化方案，以下为原始建议：\n\n{text}"),
        steps: vec![],
        error: None,
    }
}

/// 容错解析模型输出：提取首尾大括号之间的内容按 JSON 解析；失败则原文兜底。
fn parse_plan_reply(text: &str) -> RepairPlan {
    let trimmed = text.trim();
    let start = match trimmed.find('{') {
        Some(i) => i,
        None => return text_fallback(text),
    };
    let end = match trimmed.rfind('}') {
        Some(i) if i > start => i + 1,
        _ => return text_fallback(text),
    };
    match serde_json::from_str::<serde_json::Value>(&trimmed[start..end]) {
        Ok(v) => {
            let summary = v["summary"].as_str().unwrap_or("修复方案").to_string();
            let mut steps = Vec::new();
            if let Some(arr) = v["steps"].as_array() {
                for s in arr {
                    let command = s["command"].as_str().map(|c| c.trim().to_string()).filter(|c| !c.is_empty());
                    // isExecutable 由后端按白名单裁决，不信任模型自述
                    let is_executable =
                        command.as_deref().map(|c| resource::validate_command(c).is_ok()).unwrap_or(false);
                    steps.push(PlanStep {
                        title: s["title"].as_str().unwrap_or("未命名步骤").to_string(),
                        detail: s["detail"].as_str().unwrap_or("").to_string(),
                        command,
                        is_executable,
                    });
                }
            }
            if steps.is_empty() {
                return RepairPlan {
                    ok: true,
                    mode: "text".into(),
                    summary: format!("{summary}\n\n{text}"),
                    steps: vec![],
                    error: None,
                };
            }
            RepairPlan {
                ok: true,
                mode: "ai".into(),
                summary,
                steps,
                error: None,
            }
        }
        Err(_) => RepairPlan {
            ok: true,
            mode: "text".into(),
            summary: format!("AI 返回内容无法解析为结构化方案，以下为原始建议：\n\n{text}"),
            steps: vec![],
            error: None,
        },
    }
}

/// POST /api/env/repair/plan —— 已配置 AI 助手时生成结构化修复方案。
/// 输入当前全部问题项（fail/warn），后端附带资源目录清单做上下文。
pub async fn repair_plan(
    State(state): State<Arc<AppState>>,
    Json(input): Json<RepairPlanInput>,
) -> Json<RepairPlan> {
    if input.check_items.is_empty() {
        return Json(RepairPlan {
            ok: false,
            mode: "none".into(),
            summary: String::new(),
            steps: vec![],
            error: Some("没有需要修复的问题项".into()),
        });
    }
    let llm = state.ai_models.lock().unwrap().active();
    let Some(llm) = llm else {
        return Json(RepairPlan {
            ok: false,
            mode: "none".into(),
            summary: String::new(),
            steps: vec![],
            error: Some("尚未配置 AI 助手，请先在「AI 助手」页配置模型".into()),
        });
    };

    let dir = state.app_settings.lock().unwrap().resource_dir.clone();
    let files = blocking(move || resource::scan(&dir).files).await;
    // 已安装状态：让 AI 区分「全新安装」与「修复/重装」，方案更贴近实情
    let mut installed = Vec::new();
    for t in UNINSTALLABLE {
        let (on, _, _) = probe_target(&state, t).await;
        installed.push((resource::target_label(t).to_string(), on));
    }
    let prompt = build_plan_prompt(
        &input.check_items,
        &files,
        &installed,
        input.previous_plan.as_deref(),
        input.follow_up.as_deref(),
    );

    match ai::analyze(&llm, prompt).await {
        Ok(text) => Json(parse_plan_reply(&text)),
        Err(e) => Json(RepairPlan {
            ok: false,
            mode: "none".into(),
            summary: String::new(),
            steps: vec![],
            error: Some(format!("AI 请求失败：{e}")),
        }),
    }
}

// ===== 白名单代执行 =====

#[derive(Debug, Deserialize)]
pub struct ExecInput {
    pub command: String,
}

#[derive(Debug, Serialize)]
pub struct ExecResult {
    pub ok: bool,
    pub error: Option<String>,
}

/// POST /api/env/repair/exec —— 白名单代执行（UAC 提权，单条命令）。
///
/// 安全边界：
/// - validate_command：首 token 必须在白名单（msiexec/winget/choco/sc/net/netsh/
///   robocopy/xcopy/curl），且禁止 `; | & ` ` $` 等元字符——AI 生成的命令不可信；
/// - 通过后再把命令拆为「程序 + 参数串」交给 Start-Process -Verb RunAs，
///   参数串放 PowerShell 单引号字面量（无插值、无 shell 解释层），用户在 UAC
///   弹窗点击确认 = 授权动作。
pub async fn repair_exec(
    State(_state): State<Arc<AppState>>,
    Json(input): Json<ExecInput>,
) -> Json<ExecResult> {
    if let Err(e) = resource::validate_command(&input.command) {
        return Json(ExecResult {
            ok: false,
            error: Some(e),
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = input;
        return Json(ExecResult {
            ok: false,
            error: Some("代执行仅支持 Windows 目标机，请复制命令手动执行".into()),
        });
    }
    #[cfg(target_os = "windows")]
    {
        let cmd = input.command.trim().to_string();
        // 拆首 token（支持带引号的 "msiexec.exe"）与剩余参数（保留原始引号）
        let (exe, rest) = if let Some(stripped) = cmd.strip_prefix('"') {
            match stripped.find('"') {
                Some(i) => (
                    stripped[..i].to_string(),
                    stripped[i + 1..].trim().to_string(),
                ),
                None => (stripped.to_string(), String::new()),
            }
        } else {
            match cmd.find(char::is_whitespace) {
                Some(i) => (cmd[..i].to_string(), cmd[i..].trim().to_string()),
                None => (cmd.clone(), String::new()),
            }
        };
        let exe = exe.replace('\'', "''");
        let rest = rest.replace('\'', "''");
        let ps = if rest.is_empty() {
            format!(
                "$p = Start-Process -FilePath '{exe}' -Verb RunAs -Wait -PassThru; if ($p) {{ exit $p.ExitCode }} else {{ exit 1 }}"
            )
        } else {
            format!(
                "$p = Start-Process -FilePath '{exe}' -ArgumentList '{rest}' -Verb RunAs -Wait -PassThru; if ($p) {{ exit $p.ExitCode }} else {{ exit 1 }}"
            )
        };
        // 安装类命令可能持续数分钟（MySQL MSI / winget 下载），10 分钟硬超时兜底
        let run = tokio::time::timeout(
            std::time::Duration::from_secs(600),
            tokio::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps])
                .kill_on_drop(true)
                .output(),
        )
        .await;
        match run {
            Ok(Ok(out)) if out.status.success() => Json(ExecResult {
                ok: true,
                error: None,
            }),
            Ok(Ok(out)) => {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                let msg = if stderr.is_empty() {
                    format!("命令执行失败（退出码 {:?}）", out.status.code())
                } else {
                    format!("命令执行失败：{stderr}")
                };
                Json(ExecResult {
                    ok: false,
                    error: Some(msg),
                })
            }
            Ok(Err(e)) => Json(ExecResult {
                ok: false,
                error: Some(format!("无法启动命令：{e}")),
            }),
            Err(_) => Json(ExecResult {
                ok: false,
                error: Some("命令执行超时（10 分钟），请检查目标安装器是否在等待界面输入".into()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AI 输出容错解析：干净 JSON / markdown 包裹 / 纯文本 / 缺 steps 各分支。
    #[test]
    fn parse_plan_reply_handles_llm_variants() {
        // 干净 JSON：isExecutable 由后端白名单裁决
        let plan = parse_plan_reply(
            r#"{"summary":"安装 MySQL","steps":[{"title":"静默安装","detail":"x","command":"msiexec /i \"C:/res/mysql.msi\" /quiet","needsAdmin":true},{"title":"复制执行","detail":"y","command":"powershell -c evil"}]}"#,
        );
        assert_eq!(plan.mode, "ai");
        assert_eq!(plan.steps.len(), 2);
        assert!(plan.steps[0].is_executable, "白名单内命令应可代执行");
        assert!(!plan.steps[1].is_executable, "powershell 不在白名单");

        // markdown 代码块包裹
        let wrapped = "```json\n{\"summary\":\"s\",\"steps\":[{\"title\":\"t\",\"command\":\"sc query MySQL80\"}]}\n```";
        let plan = parse_plan_reply(wrapped);
        assert_eq!(plan.mode, "ai");
        assert!(plan.steps[0].is_executable);

        // 纯文本兜底：不 panic，原文进 summary
        let plan = parse_plan_reply("请手动安装 MySQL 并启动服务。");
        assert_eq!(plan.mode, "text");
        assert!(plan.summary.contains("手动安装"));

        // 非法 JSON（截断的大括号）兜底
        let plan = parse_plan_reply("{\"summary\":\"s\", \"steps\": [");
        assert_eq!(plan.mode, "text");

        // 合法 JSON 但无 steps：原文兜底
        let plan = parse_plan_reply("{\"summary\":\"只有概述\"}");
        assert_eq!(plan.mode, "text");
        assert!(plan.summary.contains("只有概述"));
    }

    /// prompt 必须携带问题项、资源清单与白名单约定；多轮追问时带上一轮方案；
    /// 已安装状态也会并入（让 AI 区分全新安装与修复）。
    #[test]
    fn plan_prompt_contains_context() {
        let items = vec![PlanCheckItem {
            id: "dep-mysql".into(),
            name: "MySQL".into(),
            status: "fail".into(),
            expected: "3306 可达".into(),
            actual: "未监听".into(),
            fix_hint: "winget install".into(),
            category: "外部依赖".into(),
        }];
        let files = vec![resource::ResourceFile {
            file_name: "mysql-installer-community-8.0.42.0.msi".into(),
            path: "C:/res/mysql.msi".into(),
            size_bytes: 300 * 1024 * 1024,
            target: Some("mysql".into()),
            in_convention_dir: true,
        }];
        let installed = vec![
            ("MySQL".to_string(), false),
            ("Redis".to_string(), true),
        ];
        let p = build_plan_prompt(&items, &files, &installed, None, None);
        assert!(p.contains("dep-mysql") && p.contains("3306 可达"));
        assert!(p.contains("mysql-installer-community-8.0.42.0.msi"));
        assert!(p.contains("msiexec"));
        assert!(p.contains("MySQL：未安装") && p.contains("Redis：已安装"));
        let p_empty = build_plan_prompt(&items, &[], &installed, None, None);
        assert!(p_empty.contains("未配置或没有识别出的安装包"));
        let p_follow = build_plan_prompt(&items, &files, &installed, Some("上一轮：装 MySQL"), Some("端口还是不通"));
        assert!(p_follow.contains("上一轮：装 MySQL") && p_follow.contains("端口还是不通"));
    }
}
