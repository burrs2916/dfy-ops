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
#[tauri::command]
fn uses_custom_titlebar() -> bool {
    cfg!(target_os = "windows")
}

/// 打开「边缘计算控制台」独立原生窗口：控制台页面（edge-conductor 托管的
/// /console/ 静态产物）不用挤在平台主窗口的 iframe 里，像独立应用一样有
/// 自己的任务栏图标。前端传入拼好的完整 URL（含 cloudApi= 查询参数）。
/// 同 label 窗口已存在则仅前置聚焦，不重复开。
#[tauri::command]
fn open_console_window(app: tauri::AppHandle, url: String) -> Result<(), String> {
    // 安全校验：只允许打开本机回环的 /console/ 页面，防止被拿来开任意远程站点
    let parsed = tauri::Url::parse(&url).map_err(|e| format!("控制台地址不合法: {e}"))?;
    if parsed.host_str() != Some("127.0.0.1") || !parsed.path().starts_with("/console/") {
        return Err("仅支持打开本机边缘控制台".into());
    }
    // 已开着 → 前置聚焦即可（不重复开一堆窗口）
    if let Some(w) = app.get_webview_window("edge-console") {
        let _ = w.set_focus();
        return Ok(());
    }
    tauri::webview::WebviewWindowBuilder::new(&app, "edge-console", tauri::WebviewUrl::External(parsed.clone()))
        .title("边缘计算控制台")
        .inner_size(1280.0, 860.0)
        .min_inner_size(960.0, 640.0)
        // 显式声明全部系统装饰与能力：Windows 上动态建窗口偶发按钮/拖拽失效，
        // 显式设置规避继承主窗口状态导致的交互异常
        .decorations(true)
        .resizable(true)
        .maximizable(true)
        .minimizable(true)
        .closable(true)
        .center()
        .build()
        .map(|window| {
            debug_log::entry("console-window", "info", "open", "created", &url);
            // Windows 实测：外部 URL 的 WebView2 窗口偶发「整个窗口无响应、点关闭
            // 无效」——WebView2 优雅关闭流程（等待页面 JS/网络空闲）可能无限挂起。
            // 兜底：CloseRequested 不走默认 close（等待 webview 优雅退出），
            // 直接 destroy 强制销毁窗口与 webview 进程，保证永远关得掉。
            let w = window.clone();
            window.on_window_event(move |event| match event {
                tauri::WindowEvent::CloseRequested { .. } => {
                    debug_log::entry("console-window", "warn", "close", "close-requested-force-destroy", "");
                    let _ = w.destroy();
                }
                tauri::WindowEvent::Destroyed => {
                    debug_log::entry("console-window", "info", "close", "destroyed", "");
                }
                _ => {}
            });
        })
        .map_err(|e| format!("打开控制台窗口失败: {e}"))
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

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
            debug_log::write("===== DFY Ops debug.log 启动 =====");

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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![uses_custom_titlebar, open_console_window])
        .run(tauri::generate_context!())
        .expect("error while running DFY 运维平台");
}
