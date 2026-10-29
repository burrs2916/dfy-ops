//! 环境修复：Java 运行时发现与自动安装（Windows 目标机交付场景）。
//!
//! 检测口径（与 environment.rs 的「进程是事实来源」一致）：
//! - 服务由 `run_cmd`（`java -jar …`）启动，用的是 **PATH 上的 java**——
//!   因此 PATH java 是第一事实；已装 JDK 清单只在 PATH 不达标时才盘点（省探测开销）。
//! - 清单来源：Windows 读注册表约定键 + 默认安装目录兜底；macOS 用
//!   `/usr/libexec/java_home -V`（仅 dev 机参考，交付平台是 Windows）。
//!
//! 修复分级（POST /api/env/repair 的决策链）：
//! 1. PATH java 达标 → 无需修复；
//! 2. 本机已有达标 JDK → 返回配置指引（设 JAVA_HOME / 加 PATH），不擅自改；
//! 3. 纯净机什么都没有 → 自动安装：下载 Adoptium Temurin JRE MSI →
//!    UAC 提权静默安装（MSI 的 FeatureJavaHome + FeatureEnvironment 自动写
//!    JAVA_HOME 与 PATH，无需手工改注册表）→ 复探验证。

use serde::Serialize;
use std::sync::{Arc, Mutex};

pub mod jdk;
pub mod install;
pub mod uninstall;

pub use jdk::*;
pub use install::*;
pub use uninstall::*;


/// 修复任务状态（全局单例；前端轮询 GET /api/env/repair/status）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct RepairJob {
    pub running: bool,
    /// download → install → verify → done
    pub stage: String,
    /// 任务类别（install/uninstall）：stage 完成时会被置为 done，
    /// 区分安装/卸载要用这个字段
    pub op: String,
    /// 修复目标软件（java/mysql/redis/vernemq/…；None = 旧接口未标注）
    pub target: Option<String>,
    pub logs: Vec<String>,
    pub ok: Option<bool>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct RepairState {
    job: Mutex<RepairJob>,
}

impl RepairState {
    pub fn snapshot(&self) -> RepairJob {
        self.job.lock().unwrap().clone()
    }
    fn patch(&self, f: impl FnOnce(&mut RepairJob)) {
        let mut job = self.job.lock().unwrap();
        f(&mut job);
        if job.logs.len() > 200 {
            let drain = job.logs.len() - 200;
            job.logs.drain(..drain);
        }
    }
    /// 追加任务日志（修复中心进度区展示；上限 200 条自动截断）。
    /// 同时镜像写 debug.log（scope=Repair）：job.logs 每次安装/卸载都会清空，
    /// 不镜像的话「安装失败现场」在 UI 上几秒内就消失——用户传回 debug.log
    /// 时必须能看到完整安装过程（解压/配置/注册服务/验证每一步）。
    pub fn log(&self, msg: impl Into<String>) {
        let msg = msg.into();
        crate::debug_log::entry(
            crate::debug_log::APP_SCOPE,
            "INFO",
            "repair_job",
            "log",
            &msg,
        );
        self.patch(move |j| j.logs.push(msg));
    }
}

/// nginx.exe 是否在跑（tasklist；安装自动启动复核与运维层共用，状态同源）。
#[cfg(target_os = "windows")]
pub fn nginx_running() -> bool {
    std::process::Command::new("tasklist")
        .args(["/fi", "imagename eq nginx.exe", "/fo", "csv", "/nh"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("nginx.exe"))
        .unwrap_or(false)
}

/// 非 Windows（dev 机）：无 nginx 进程托管，恒 false。
#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn nginx_running() -> bool {
    false
}

/// 环境检查全量缓存类型（修复完成后清空，让下一次 /api/env 重新实测而非用 60s 旧缓存）。
pub type EnvCache = Arc<Mutex<Option<(std::time::Instant, Vec<crate::ops::environment::ServiceEnvReport>)>>>;

/// 平台管理的 JDK 免安装目录（zip 解压即用，无 MSI 产品、无注册表残留；
/// 与 mysql/redis 的 C:\services 同风格）。检测优先级最高：有 bin\java.exe 即达标。
pub const JDK_MANAGED_HOME: &str = "C:\\services\\jdk";

/// 免安装版 JDK 的默认最低大版本（一键修复对 java 的预期，与注册表最低声明一致）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub const JDK_DEFAULT_MAJOR: u32 = 8;


/// 收尾：更新任务终态 + 完成事件落 ops-events.jsonl + 清空环境检查缓存。
///
/// 所有安装/卸载管线（zip/jar/msi/jdk 在线）都汇聚到这里——完成事件一处记录
/// 即全管线覆盖。失败原因一并入事件（截 160 字），审计时间轴上「谁装了什么、
/// 成没成、为什么失败」无需翻 debug.log。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
async fn finish(state: Arc<RepairState>, env_cache: EnvCache, ok: bool, error: String) {
    // op/target 在 patch 前取快照（patch 不动这两个字段，但显式先取语义清晰）
    let (op, target) = {
        let job = state.job.lock().unwrap();
        (job.op.clone(), job.target.clone())
    };
    if !error.is_empty() {
        state.log(format!("失败：{error}"));
    }
    state.patch(|j| {
        j.running = false;
        j.stage = "done".into();
        j.ok = Some(ok);
        j.error = if error.is_empty() { None } else { Some(error.clone()) };
    });
    // 完成事件：actor 按操作类别区分——安装是管线行为（installer），
    // 卸载由用户在界面发起（user）；action 直接用 op（install/uninstall）
    if !op.is_empty() {
        if let Some(t) = &target {
            let detail = if error.is_empty() {
                "任务完成".to_string()
            } else {
                format!("任务失败：{}", &error.chars().take(160).collect::<String>())
            };
            let actor = if op == "uninstall" { "user" } else { "installer" };
            crate::ops_events::record(actor, t, &op, ok, &detail);
        }
    }
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(mut guard) = env_cache.lock() {
            *guard = None;
        }
    })
    .await;
}
