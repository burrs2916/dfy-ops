mod api;
mod ops;

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
        .run(tauri::generate_context!())
        .expect("error while running DFY 运维平台");
}
