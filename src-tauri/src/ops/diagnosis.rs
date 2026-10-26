use serde::Serialize;
use std::collections::HashMap;

use crate::ops::environment::{CheckStatus, ServiceEnvReport};
use crate::ops::registry::ServiceDef;

/// 诊断严重级别。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Critical,
}

/// 一条诊断发现。
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub service: String,
    pub severity: Severity,
    pub message: String,
    /// 缺失依赖时的修复建议（面向 Windows 的 winget/choco/下载地址等），
    /// 供用户在运维中心自助修复。仅当该发现由“环境缺失/警告”产生时携带。
    #[serde(rename = "fixHint")]
    pub fix_hint: Option<String>,
}

/// 规则诊断引擎（兜底层）。
///
/// status: 服务 id -> (是否在运行, 是否健康)。
/// envs:   各服务的环境检查报告（工具链 + 外部依赖）。
///
/// 两阶段：
///  1) 运行态诊断 —— 常驻服务是否在跑 / 端口是否可达 / 依赖是否就绪。
///  2) 环境诊断   —— 依赖缺失（这是“环境没装好”这一类最关键的问题），
///     缺失即阻断服务运行，之前被完全忽略，现纳入发现项并附带修复命令。
pub fn diagnose(
    defs: &[ServiceDef],
    status: &HashMap<String, (bool, bool)>,
    envs: &[ServiceEnvReport],
) -> Vec<Finding> {
    let mut out = Vec::new();

    // 阶段 1：运行态诊断
    for d in defs {
        let (running, healthy) = status.get(&d.id).copied().unwrap_or((false, false));

        if d.is_daemon {
            if !running {
                out.push(Finding {
                    service: d.id.clone(),
                    severity: Severity::Warning,
                    message: format!("服务 {} 未运行（无存活进程）", d.name),
                    fix_hint: None,
                });
            } else if !healthy {
                out.push(Finding {
                    service: d.id.clone(),
                    severity: Severity::Critical,
                    message: format!(
                        "服务 {} 进程在运行但端口不可达，可能启动失败或端口冲突",
                        d.name
                    ),
                    fix_hint: None,
                });
            }
        }

        for dep in &d.depends_on {
            let dep_running = status.get(dep).map(|s| s.0).unwrap_or(false);
            if !dep_running {
                out.push(Finding {
                    service: d.id.clone(),
                    severity: Severity::Warning,
                    message: format!("依赖服务 {dep} 未运行，可能影响 {}", d.name),
                    fix_hint: None,
                });
            }
        }
    }

    // 阶段 2：环境诊断（依赖缺失 / 警告）
    for env in envs {
        for item in &env.items {
            let fix = if item.fix_hint.is_empty() {
                None
            } else {
                Some(item.fix_hint.clone())
            };
            match item.status {
                CheckStatus::Fail => out.push(Finding {
                    service: env.id.clone(),
                    severity: Severity::Critical,
                    message: format!(
                        "环境缺失（致命）：{} —— 实际：{}；需先在运维中心修复后方可运行",
                        item.name, item.actual
                    ),
                    fix_hint: fix,
                }),
                CheckStatus::Warn => out.push(Finding {
                    service: env.id.clone(),
                    severity: Severity::Warning,
                    message: format!(
                        "环境警告：{} —— 实际：{}；可能影响部分功能",
                        item.name, item.actual
                    ),
                    fix_hint: fix,
                }),
                CheckStatus::Pass => {}
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::environment::{CheckItem, CheckStatus};
    use std::collections::HashMap;

    /// 验证环境缺失 / 警告被正确转化为诊断发现项，并携带修复命令。
    #[test]
    fn diagnose_env_missing_becomes_finding() {
        let items = vec![
            CheckItem {
                id: "java-runtime".into(),
                name: "Java 运行时".into(),
                category: "工具链".into(),
                status: CheckStatus::Fail,
                expected: "JDK 8+".into(),
                actual: "未检测到".into(),
                is_fatal: true,
                fix_hint: "winget install -e --id Microsoft.OpenJDK.17".into(),
            },
            CheckItem {
                id: "nginx-hosting".into(),
                name: "nginx 托管".into(),
                category: "外部依赖".into(),
                status: CheckStatus::Warn,
                expected: "已安装".into(),
                actual: "未检测到".into(),
                is_fatal: false,
                fix_hint: "winget install -e --id nginx.nginx".into(),
            },
            CheckItem {
                id: "nodejs".into(),
                name: "Node.js".into(),
                category: "工具链".into(),
                status: CheckStatus::Pass,
                expected: "16+".into(),
                actual: "v20.0.0".into(),
                is_fatal: true,
                fix_hint: String::new(),
            },
        ];

        let env = ServiceEnvReport {
            id: "svc-x".into(),
            name: "测试服务".into(),
            overall: CheckStatus::Fail,
            items,
        };

        let findings = diagnose(&[], &HashMap::new(), &[env]);
        // pass 项不计入，fail + warn = 2 条
        assert_eq!(findings.len(), 2);

        let fail = findings
            .iter()
            .find(|f| matches!(f.severity, Severity::Critical))
            .expect("应有致命发现");
        assert_eq!(fail.service, "svc-x");
        assert_eq!(fail.fix_hint.as_deref(), Some("winget install -e --id Microsoft.OpenJDK.17"));

        let warn = findings
            .iter()
            .find(|f| matches!(f.severity, Severity::Warning))
            .expect("应有警告发现");
        assert_eq!(warn.fix_hint.as_deref(), Some("winget install -e --id nginx.nginx"));
    }
}
