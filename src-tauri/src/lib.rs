mod api;
mod debug_log;
mod ops;
pub mod ops_events;

#[cfg(test)]
mod contract_sync;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use api::server::{router, AppState};
use ops::aimodels::AiModelConfig;
use ops::appsettings::AppSettings;
use ops::process::ProcManager;
use ops::registry;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

/// macOS dev 模式：`tauri dev` 跑的是裸二进制（没有 .app 包壳），Dock 只显示
/// 系统通用图标，icon.icns 要 `tauri build` 出包后才生效。这里在启动时用
/// AppKit 直接把 Dock 图标设为白底 icon.png（仅 debug 构建编译，release 走 bundle icns）。
#[cfg(all(target_os = "macos", debug_assertions))]
fn set_dock_icon_dev() {
    use objc2::MainThreadMarker;
    use objc2::AnyThread;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSString;

    let icon_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("icons/icon.png");
    let Some(path_str) = icon_path.to_str() else {
        return;
    };
    let file = NSString::from_str(path_str);
    let Some(image) = NSImage::initWithContentsOfFile(NSImage::alloc(), &file) else {
        tracing::warn!("dev Dock 图标加载失败: {path_str}");
        return;
    };
    let mtm = MainThreadMarker::new().expect("set_dock_icon_dev 必须在主线程调用（tauri setup 即主线程）");
    unsafe {
        NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image));
    }
    tracing::info!("dev Dock 图标已设置: {path_str}");
}

/// 前端据此决定是否渲染自绘标题栏：仅 Windows 关闭系统窗口装饰改自绘——
/// 原生标题栏左上角的系统小图标与界面 AppBar 已有的品牌大 logo 重复，且
/// 整条原生标题栏与主题风格割裂。macOS 保持系统原生标题栏（红绿灯），恒 false。
///
/// 同时是主线程存活锚点：本命令是前端渲染期发出的第一个 IPC。若 debug.log
/// 出现这行 → 主线程此刻仍可响应，卡死发生在其后；完全没有这行 → 卡在
/// WebView 初始化之前。窗口卡死问题按此二分定位。
#[tauri::command]
fn uses_custom_titlebar() -> bool {
    static ANCHOR: std::sync::Once = std::sync::Once::new();
    ANCHOR.call_once(|| {
        debug_log::write("[anchor] 主线程 IPC 可达（uses_custom_titlebar 已响应）")
    });
    cfg!(target_os = "windows")
}

/// 解析 debug.log 写入路径（和 biosphere-terminal-app 的 resolve_debug_log_paths 完全同策略）：
/// 多路径写入，总有一处能写。
///
/// 1. dirs_next::data_local_dir()
///    跨平台主路径，永远可写
///    Windows: %LOCALAPPDATA%\dfy-ops\debug.log
///    macOS:   ~/Library/Application Support/dfy-ops/debug.log
/// 2. APPDATA 兜底
///    Windows 仅当 data_local_dir 失败时用
/// 3. exe 同级目录
///    用户直觉找的位置，Program Files 下可能无权限
fn resolve_debug_log_paths() -> debug_log::DebugLogPaths {
    let mut paths: Vec<PathBuf> = Vec::new();

    // 1. data_local_dir（最稳的主路径）
    if let Some(local) = dirs_next::data_local_dir() {
        paths.push(local.join("dfy-ops").join("debug.log"));
    }

    // 2. APPDATA 兜底（Windows）
    if let Ok(appdata) = std::env::var("APPDATA") {
        let p = PathBuf::from(&appdata).join("dfy-ops").join("debug.log");
        if !paths.contains(&p) {
            paths.push(p);
        }
    }

    // 3. exe 同级目录
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
        let p = exe_dir.join("debug.log");
        if !paths.contains(&p) {
            paths.push(p);
        }
    }

    // 4. 兜底：当前工作目录
    if paths.is_empty() {
        paths.push(PathBuf::from("debug.log"));
    }

    std::sync::Arc::new(paths)
}

/// 启动早期清理同名残留实例（仅 Windows）。
///
/// 场景（2026-11-10 线上反馈）：Windows 上出现「两条标题栏、一打开就未响应、
/// 全部点击无反应」——典型的**旧实例僵死未退、新实例窗口又叠在其上**：
/// 旧实例主线程卡死（任务管理器显示未响应），其窗口却还压在上层拦截输入，
/// 用户点到的其实是那个僵尸窗口。
///
/// 策略：按镜像名枚举同名进程（开发态 exe 名 dfy-ops.exe 与发布态
/// 「DFY运维平台.exe」不同名，互不误杀），排除自己后 taskkill /F 强杀，
/// 有界等待（3s）确认退干净；杀不掉也继续启动（best-effort，不留死循环）。
/// 所有动作写入 debug.log，下次定位无需猜。
#[cfg(target_os = "windows")]
fn enforce_single_instance() {
    use std::process::Command;
    use std::time::{Duration, Instant};

    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let Some(name) = exe.file_name().and_then(|s| s.to_str()) else {
        return;
    };
    let me = std::process::id();

    let list = || -> Vec<String> {
        let filter = format!("IMAGENAME eq {name}");
        let out = Command::new("tasklist")
            .args(["/FI", filter.as_str(), "/FO", "CSV", "/NH"])
            .output();
        let Ok(out) = out else {
            return Vec::new();
        };
        String::from_utf8_lossy(&out.stdout)
            .lines()
            // CSV: "name.exe","1234","Console",... —— 取第二列 pid，排除自己
            .filter_map(|l| l.split(',').nth(1))
            .map(|p| p.trim_matches('"').trim().to_string())
            .filter(|p| p.parse::<u32>().map(|n| n != me).unwrap_or(false))
            .collect()
    };

    let pids = list();
    if pids.is_empty() {
        return;
    }
    for pid in &pids {
        debug_log::write(&format!(
            "[single-instance] 检测到同名残留实例 pid={pid}（{name}），强制结束"
        ));
        let _ = Command::new("taskkill").args(["/F", "/PID", pid.as_str()]).output();
    }
    // 有界等待退干净：僵尸窗口没退就开新窗会重现「叠窗 + 未响应」
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let left = list();
        if left.is_empty() {
            debug_log::write("[single-instance] 残留实例已全部结束，继续启动");
            return;
        }
        if Instant::now() >= deadline {
            debug_log::write(&format!(
                "[single-instance] 警告：残留实例未能完全结束（剩余 pid={:?}），继续启动——若新窗口仍异常，任务管理器手动结束它",
                left
            ));
            return;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    // debug.log 最先初始化（OnceLock，setup 里的再次 init 是幂等空操作）——
    // 单实例清理发生在任何窗口创建之前，必须先有落日志的地方。
    debug_log::init_paths(resolve_debug_log_paths());
    debug_log::write("===== DFY Ops debug.log 启动 =====");
    #[cfg(target_os = "windows")]
    enforce_single_instance();

    tauri::Builder::default()
        // 原生目录/文件选择对话框（设置 → 资源目录「浏览」）
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // debug.log：多路径写入（和 biosphere-terminal-app 同策略）。
            // 总有一处能写——Windows Program Files 下 exe 旁边可能没权限，
            // 但 data_local_dir（%LOCALAPPDATA%）一定能写。
            let paths = resolve_debug_log_paths();
            debug_log::init_paths(paths.clone());
            // 运维事件审计（ops-events.jsonl）：与 debug.log 第一优先路径同目录，
            // 启停/安装/卸载事件全部留痕，界面可查
            if let Some(first) = paths.first() {
                ops_events::init_path(first.parent().map(|d| d.to_path_buf()).unwrap_or_else(|| first.clone()));
            }
            // 在 tracing 里打出所有路径，用户从 stdout 就能看到写哪了
            for (i, p) in paths.iter().enumerate() {
                tracing::info!("[debug.log] 路径 {}：{}", i + 1, p.display());
            }
            // boot 行已在 run() 最开头写入（单实例清理前）；此处不再重复写。
            debug_log::write("===== DFY Ops debug.log 启动 =====（setup）");

            // panic hook：panic 时也写入 debug.log，用户能拿到崩溃上下文
            let paths_for_panic = paths.clone();
            std::panic::set_hook(Box::new(move |info| {
                let msg = format!("[panic] {info}");
                debug_log::write_to(&paths_for_panic, &msg);
                eprintln!("{msg}");
            }));

            #[cfg(all(target_os = "macos", debug_assertions))]
            set_dock_icon_dev();

            let config_dir = app
                .path()
                .config_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            // services.toml 必须放应用专属配置目录（app_config_dir = config_dir + identifier，
            // macOS: ~/Library/Application Support/com.dfy.ops/）。
            // 此前误用系统级 config_dir()（不带 identifier），覆盖文件放对内容却读不到。
            let app_config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| config_dir.clone());
            let procs = Arc::new(ProcManager::new());
            let state = Arc::new(AppState {
                // 本机覆盖优先：app_config_dir/services.toml > DFY_OPS_SERVICES > 内置 Windows 部署默认
                registry: registry::services_with_override(&app_config_dir),
                procs: procs.clone(),
                ai_models: Arc::new(Mutex::new(AiModelConfig::load(&config_dir))),
                app_settings: Arc::new(Mutex::new(AppSettings::load(&config_dir))),
                config_dir,
                env_cache: Arc::new(Mutex::new(None)),
                env_rebuild: Arc::new(tokio::sync::Mutex::new(())),
                repair: Arc::new(ops::env_repair::RepairState::default()),
            });

            // P2-f 保活看门狗：平台拉起过的常驻服务崩了自动重新拉起
            // （用户显式 stop 的会被抑制；重启间隔 30s 冷却防 crash-loop）。
            ops::watchdog::spawn(procs.clone(), state.registry.clone(), 15);

            std::thread::spawn(move || {
                let rt = match tokio::runtime::Runtime::new() {
                    Ok(rt) => rt,
                    Err(e) => {
                        tracing::error!("创建 tokio runtime 失败，运维 API 无法启动: {e}");
                        return;
                    }
                };
                rt.block_on(async move {
                    let app = router(state);
                    // P1-5：端口绑定失败不再 expect panic 拖垮整个桌面应用。
                    // 127.0.0.1:18721 被占用通常意味着本机已有另一个 dfy-ops 实例
                    // 在跑运维 API——只记录并退出本服务线程（即天然的「单实例」互斥）。
                    let listener = match tokio::net::TcpListener::bind("127.0.0.1:18721").await {
                        Ok(l) => l,
                        Err(e) => {
                            tracing::error!(
                                "运维 API 绑定 127.0.0.1:18721 失败（可能已有实例占用该端口）: {e}"
                            );
                            return;
                        }
                    };
                    tracing::info!("DFY Ops API listening on http://127.0.0.1:18721");
                    if let Err(e) = axum::serve(listener, app).await {
                        tracing::error!("运维 API 服务异常退出: {e}");
                    }
                });
            });

            // Windows 系统装饰在 Rust 侧兜底关闭（tauri.windows.conf.json 合并
            // 万一失效，界面会出现「系统标题栏 + 自绘标题栏」两条并存的回归）。
            // 前端 uses_custom_titlebar 按 Windows 恒渲染自绘栏，两侧口径必须一致。
            #[cfg(target_os = "windows")]
            if let Some(w) = app.get_webview_window("main") {
                if let Err(e) = w.set_decorations(false) {
                    debug_log::write(&format!("[titlebar] set_decorations(false) 失败: {e}"));
                } else {
                    debug_log::write("[titlebar] 系统装饰已确认关闭（运行时兜底）");
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![uses_custom_titlebar])
        .run(tauri::generate_context!())
        .expect("error while running DFY 运维平台");
}
