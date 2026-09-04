mod api;
mod ops;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    tauri::Builder::default()
        .setup(|app| {
            #[cfg(all(target_os = "macos", debug_assertions))]
            set_dock_icon_dev();

            let config_dir = app
                .path()
                .config_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            let state = Arc::new(AppState {
                registry: registry::services(),
                procs: ProcManager::new(),
                ai_models: Arc::new(Mutex::new(AiModelConfig::load(&config_dir))),
                app_settings: Arc::new(Mutex::new(AppSettings::load(&config_dir))),
                config_dir,
            });

            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
                rt.block_on(async move {
                    let app = router(state);
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:18721")
                        .await
                        .expect("failed to bind ops API on 127.0.0.1:18721");
                    tracing::info!("DFY Ops API listening on http://127.0.0.1:18721");
                    axum::serve(listener, app)
                        .await
                        .expect("ops API server crashed");
                });
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running DFY 运维平台");
}
