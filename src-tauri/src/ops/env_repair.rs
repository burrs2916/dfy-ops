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

/// 本机一个已安装的 Java 运行时（home 目录级）。
#[derive(Debug, Clone)]
pub struct JavaInstall {
    /// JAVA_HOME 级目录（bin/java 的上一级）
    pub home: String,
    /// `java -version` 解析结果；None = 无法解析（仍列出，供展示）
    pub version: Option<(u32, u32, u32)>,
}

impl JavaInstall {
    pub fn major(&self) -> u32 {
        self.version.map(|(m, _, _)| m).unwrap_or(0)
    }
    pub fn version_label(&self) -> String {
        match self.version {
            Some((a, b, c)) => format!("{a}.{b}.{c}"),
            None => "版本未知".into(),
        }
    }
}

/// 探测某个 home 的 java 版本（`<home>/bin/java -version`，输出在 stderr）。
fn probe_home_version(home: &str) -> Option<(u32, u32, u32)> {
    let exe = if cfg!(target_os = "windows") {
        format!("{home}\\bin\\java.exe")
    } else {
        format!("{home}/bin/java")
    };
    let out = std::process::Command::new(exe).arg("-version").output().ok()?;
    let text = if out.stderr.is_empty() {
        String::from_utf8_lossy(&out.stdout).to_string()
    } else {
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    crate::ops::environment::java_version_tuple(&text)
}

/// 解析 `/usr/libexec/java_home -V` 输出（stderr），提取每行末尾的 Home 路径。
/// 行样例：`    17.0.15 (arm64) "Homebrew" - "OpenJDK 17.0.15" /opt/homebrew/.../Home`
fn parse_java_home_v(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            // 有效行含 "(arch)" 标记与末尾路径；"Matching Java Virtual Machines" 表头被过滤
            if !l.contains('(') {
                return None;
            }
            l.split_whitespace().last().map(|s| s.to_string())
        })
        .filter(|p| p.starts_with('/'))
        .collect()
}

/// 列出本机全部 Java 运行时（去重、逐个探测版本；探测失败的目录剔除）。
#[cfg(target_os = "windows")]
pub fn installed_jdks() -> Vec<JavaInstall> {
    use std::collections::BTreeSet;
    use winreg::enums::HKEY_LOCAL_MACHINE;
    use winreg::RegKey;

    let mut homes: BTreeSet<String> = BTreeSet::new();
    // 注册表约定键：Oracle/Adoptium/Microsoft 安装器都会写 JavaHome（或 Path）指向 home 目录
    for sub in [
        r"SOFTWARE\JavaSoft\Java Runtime Environment",
        r"SOFTWARE\JavaSoft\JDK",
        r"SOFTWARE\JavaSoft\JRE",
        r"SOFTWARE\Eclipse Adoptium",
    ] {
        let Ok(root) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(sub) else {
            continue;
        };
        for name in root.enum_keys().flatten() {
            let Ok(k) = root.open_subkey(&name) else { continue };
            for val in ["JavaHome", "Path"] {
                if let Ok(p) = k.get_value::<String, _>(val) {
                    if !p.is_empty() {
                        homes.insert(p);
                    }
                }
            }
        }
    }
    // 目录兜底：MSI 默认安装位置（覆盖注册表键写法差异/权限受限场景）
    for dir in [
        r"C:\Program Files\Eclipse Adoptium",
        r"C:\Program Files\Java",
        r"C:\Program Files\Microsoft",
    ] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                if e.path().is_dir() {
                    homes.insert(e.path().to_string_lossy().to_string());
                }
            }
        }
    }
    homes
        .into_iter()
        .filter_map(|home| {
            let version = probe_home_version(&home)?;
            Some(JavaInstall {
                home,
                version: Some(version),
            })
        })
        .collect()
}

/// macOS（dev 机参考）：`/usr/libexec/java_home -V` 列出全部 JVM。
#[cfg(target_os = "macos")]
pub fn installed_jdks() -> Vec<JavaInstall> {
    let Ok(out) = std::process::Command::new("/usr/libexec/java_home").arg("-V").output() else {
        return vec![];
    };
    let text = String::from_utf8_lossy(&out.stderr);
    parse_java_home_v(&text)
        .into_iter()
        .filter_map(|home| {
            let version = probe_home_version(&home)?;
            Some(JavaInstall {
                home,
                version: Some(version),
            })
        })
        .collect()
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn installed_jdks() -> Vec<JavaInstall> {
    vec![]
}

/// 在已装清单里找版本达标（>= expected）的 JDK，取最高版本。
pub fn find_compatible(expected_major: u32) -> Option<JavaInstall> {
    installed_jdks()
        .into_iter()
        .filter(|j| j.major() >= expected_major)
        .max_by_key(|j| j.version)
}

/// 修复任务状态（全局单例；前端轮询 GET /api/env/repair/status）。
#[derive(Debug, Clone, Serialize, Default)]
pub struct RepairJob {
    pub running: bool,
    /// download → install → verify → done
    pub stage: String,
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
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    fn log(&self, msg: impl Into<String>) {
        let msg = msg.into();
        self.patch(move |j| j.logs.push(msg));
    }
}

/// 环境检查全量缓存类型（修复完成后清空，让下一次 /api/env 重新实测而非用 60s 旧缓存）。
pub type EnvCache = Arc<Mutex<Option<(std::time::Instant, Vec<crate::ops::environment::ServiceEnvReport>)>>>;

#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const ADOPTIUM_INSTALLER_URL: &str =
    "https://api.adoptium.net/v3/installer/latest/{major}/ga/windows/x64/jre/hotspot/normal/eclipse";

/// 启动 JDK 自动安装（仅 Windows 目标机）。
/// 返回 Err 表示无法启动（已有任务在跑 / 平台不支持）。
#[cfg(target_os = "windows")]
pub fn start_jdk_install(
    expected_major: u32,
    env_cache: EnvCache,
    state: Arc<RepairState>,
) -> Result<(), String> {
    {
        let mut job = state.job.lock().unwrap();
        if job.running {
            return Err("已有修复任务在执行，请等待其完成".into());
        }
        job.running = true;
        job.stage = "download".into();
        job.ok = None;
        job.error = None;
        job.logs.clear();
    }
    tokio::spawn(async move {
        run_install(state, expected_major, env_cache).await;
    });
    Ok(())
}

/// 非 Windows 平台：dev 机不提供自动安装（交付平台是 Windows）。
#[cfg(not(target_os = "windows"))]
pub fn start_jdk_install(
    _expected_major: u32,
    _env_cache: EnvCache,
    _state: Arc<RepairState>,
) -> Result<(), String> {
    Err("自动安装仅支持 Windows 目标机；当前平台请手动安装 Java 运行时".into())
}

/// Windows 安装流水线：下载 MSI → UAC 提权静默安装 → 复探验证。
#[cfg(target_os = "windows")]
async fn run_install(state: Arc<RepairState>, expected_major: u32, env_cache: EnvCache) {
    state.log(format!(
        "开始修复：目标 JDK {expected_major}+（Eclipse Temurin JRE，Adoptium 官方源）"
    ));

    // ===== 1. 下载 MSI =====
    state.patch(|j| j.stage = "download".into());
    let url = ADOPTIUM_INSTALLER_URL.replace("{major}", &expected_major.to_string());
    let msi = std::env::temp_dir().join(format!("dfy-ops-temurin{expected_major}-jre.msi"));
    let dl = async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(900))
            .build()
            .map_err(|e| format!("HTTP 客户端初始化失败：{e}"))?;
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("下载失败（网络不可达？）：{e}"))?
            .error_for_status()
            .map_err(|e| format!("下载失败：{e}"))?;
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| format!("下载中断：{e}"))?;
        std::fs::write(&msi, &bytes).map_err(|e| format!("写入临时文件失败：{e}"))?;
        Ok(bytes.len() as u64)
    };
    match dl.await {
        Ok(size) => state.log(format!("安装包下载完成：{}（{} MB）", msi.display(), size / 1024 / 1024)),
        Err(e) => {
            finish(state, env_cache, false, e).await;
            return;
        }
    }

    // ===== 2. UAC 提权静默安装（用户在 UAC 弹窗确认 = 授权动作） =====
    state.patch(|j| {
        j.stage = "install".into();
        j.logs.push(
            "正在启动 MSI 静默安装（FeatureJavaHome + FeatureEnvironment 会自动配置 JAVA_HOME 与 PATH）…".into(),
        );
        j.logs.push("请在弹出的 UAC 管理员确认框中点击「是」…".into());
    });
    let state_cl = state.clone();
    let install = tokio::task::spawn_blocking(move || {
        let ps = format!(
            "Start-Process msiexec -ArgumentList '/i \"{}\" /quiet ADDLOCAL=FeatureMain,FeatureEnvironment,FeatureJarFileRunWith,FeatureJavaHome' -Verb RunAs -Wait",
            msi.display()
        );
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &ps])
            .output()
    })
    .await;
    match install {
        Ok(Ok(out)) if out.status.success() => {
            state.log("MSI 安装完成".into());
        }
        Ok(Ok(out)) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            finish(
                state,
                env_cache,
                false,
                format!("安装执行失败（可能用户在 UAC 中点了取消）：{stderr}"),
            )
            .await;
            return;
        }
        Ok(Err(e)) => {
            finish(state, env_cache, false, format!("无法启动安装器：{e}")).await;
            return;
        }
        Err(e) => {
            finish(state, env_cache, false, format!("安装任务异常：{e}")).await;
            return;
        }
    }

    // ===== 3. 复探验证（新装 PATH 对本进程不可见，必须走注册表/目录扫描 + 绝对路径探测） =====
    state.patch(|j| j.stage = "verify".into());
    let found = tokio::task::spawn_blocking(move || find_compatible(expected_major))
        .await
        .ok()
        .flatten();
    match found {
        Some(j) => {
            state.log(format!(
                "验证通过：检测到 JDK {}（{}）已安装并配置环境变量",
                j.version_label(),
                j.home
            ));
            finish(state, env_cache, true, String::new()).await;
        }
        None => {
            finish(
                state,
                env_cache,
                false,
                "安装流程已执行但未检测到达标 JDK，请查看安装日志或手动安装".into(),
            )
            .await;
        }
    }
}

/// 收尾：更新任务终态 + 清空环境检查缓存（下一次 /api/env 全量实测）。
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
async fn finish(state: Arc<RepairState>, env_cache: EnvCache, ok: bool, error: String) {
    if !error.is_empty() {
        state.log(format!("失败：{error}"));
    }
    state.patch(|j| {
        j.running = false;
        j.stage = "done".into();
        j.ok = Some(ok);
        j.error = if error.is_empty() { None } else { Some(error) };
    });
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(mut guard) = env_cache.lock() {
            *guard = None;
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// java_home -V 输出解析：提取 4 个 Home 路径，过滤表头行。
    #[test]
    fn parse_java_home_v_extracts_paths() {
        let sample = "Matching Java Virtual Machines (4):\n\
             \x20   21.0.8 (arm64) \"JetBrains s.r.o.\" - \"JBR-21.0.8+9-1163.62-nomod 21.0.8\" /Users/x/Library/Java/JavaVirtualMachines/jbr-21.0.8/Contents/Home\n\
             \x20   17.0.15 (arm64) \"Homebrew\" - \"OpenJDK 17.0.15\" /opt/homebrew/Cellar/openjdk@17/17.0.15/libexec/openjdk.jdk/Contents/Home\n\
             \x20   11.0.29 (arm64) \"Microsoft\" - \"OpenJDK 11.0.29\" /Users/x/Library/Java/JavaVirtualMachines/ms-11.0.29/Contents/Home\n\
             \x20\n\
             \x20/Users/x/Library/Java/JavaVirtualMachines/jbr-21.0.8/Contents/Home\n";
        let paths = parse_java_home_v(sample);
        // 3 个 JVM 行各提取一个路径；末尾「默认 Home」行与表头行（无 (arch) 标记）被过滤——
        // 该路径与 JVM 行重复，少提取无碍（消费端按 home 去重探测）
        assert_eq!(paths.len(), 3, "应提取 3 个 JVM Home 路径: {paths:?}");
        assert!(paths[0].ends_with("jbr-21.0.8/Contents/Home"));
        assert!(paths[1].contains("openjdk@17"));
    }
}
