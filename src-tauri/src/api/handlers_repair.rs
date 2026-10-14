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
/// 1. 资源目录有该目标的 ZIP 免安装版（mysql/redis）→ 解压 → 配置 → 注册服务（可靠离线管线，
///    MySQL 官方 msi 是 Installer 束，静默执行装不上 Server，故 zip 优先）；
/// 2. 有 MSI（java/vernemq/memurai 等）→ msiexec 静默安装；
/// 3. 无包且 target=java → 在线下载 Adoptium Temurin（现有链路）；
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

    let started = if (target == "mysql" || target == "redis") && zip.is_some() {
        // 免安装版管线：解压到服务注册表声明的 installDir（如 C:/services/mysql）
        let z = zip.clone().unwrap();
        let install_dir = state
            .registry
            .iter()
            .find(|d| d.id == target)
            .map(|d| d.install_dir.clone())
            .unwrap_or_else(|| format!("C:\\services\\{target}"));
        crate::ops::env_repair::start_zip_install(
            target,
            z.path,
            install_dir,
            state.env_cache.clone(),
            state.repair.clone(),
        )
    } else if let Some(f) = msi.clone().or_else(|| {
        // 非 mysql/redis 目标没有 msi 管线时不吃 zip（java zip 不是安装器）
        if target != "mysql" && target != "redis" {
            zip.clone()
        } else {
            None
        }
    }) {
        crate::ops::env_repair::start_resource_install(
            target,
            f.path,
            state.env_cache.clone(),
            state.repair.clone(),
        )
    } else if target == "java" && zip.is_none() && msi.is_none() {
        // 无本地包的 java 在线兜底（expected 按注册表最低声明 8；
        // find_compatible 验证只升不降，装更高版本同样通过）
        crate::ops::env_repair::start_jdk_install(8, state.env_cache.clone(), state.repair.clone())
    } else {
        Err(format!(
            "本地资源目录中没有 {label} 的可安装包。请将 MSI 安装包（或 MySQL/Redis 的 ZIP 免安装版）放入「设置 → 本地资源目录」后重试，或使用 AI 方案"
        ))
    };

    let source = if target == "mysql" || target == "redis" {
        zip.as_ref().map(|_| "zip")
    } else {
        msi.as_ref().map(|_| "msi")
    };
    match started {
        Ok(()) => Json(serde_json::json!({
            "ok": true,
            "action": "install",
            "message": match source {
                Some("zip") => format!("开始离线安装 {label}（免安装版：解压 → 初始化 → 注册服务），全程只需一次管理员确认，进度见下方任务状态"),
                Some("msi") => format!("开始离线安装 {label}（{}）→ 管理员确认 → 验证，进度见下方任务状态", msi.as_ref().map(|f| f.file_name.as_str()).unwrap_or_default()),
                _ => format!("资源目录无 {label} 安装包，开始在线安装（下载 → 管理员确认 → 验证）"),
            }
        })),
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
    let prompt = build_plan_prompt(
        &input.check_items,
        &files,
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

    /// prompt 必须携带问题项、资源清单与白名单约定；多轮追问时带上一轮方案。
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
        }];
        let p = build_plan_prompt(&items, &files, None, None);
        assert!(p.contains("dep-mysql") && p.contains("3306 可达"));
        assert!(p.contains("mysql-installer-community-8.0.42.0.msi"));
        assert!(p.contains("msiexec"));
        let p_empty = build_plan_prompt(&items, &[], None, None);
        assert!(p_empty.contains("未配置或没有识别出的安装包"));
        let p_follow = build_plan_prompt(&items, &files, Some("上一轮：装 MySQL"), Some("端口还是不通"));
        assert!(p_follow.contains("上一轮：装 MySQL") && p_follow.contains("端口还是不通"));
    }
}
