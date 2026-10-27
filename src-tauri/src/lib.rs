mod api;
mod debug_log;
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

/// 前端据此决定是否渲染自绘标题栏：仅 Windows 关闭系统窗口装饰改自绘——
/// 原生标题栏左上角的系统小图标与界面 AppBar 已有的品牌大 logo 重复，且
/// 整条原生标题栏与主题风格割裂。macOS 保持系统原生标题栏（红绿灯），恒 false。
#[tauri::command]
fn uses_custom_titlebar() -> bool {
    cfg!(target_os = "windows")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    // debug.log：会话级诊断文件，写在 exe 同级目录。tracing 打 stdout 给开发看，
    // debug.log 打持久文件给用户提交 bug 用。两者并行，互不替代。
    let log_path = debug_log::resolve_default_path();
    let path_display = log_path.display().to_string();
    debug_log::init(log_path);
    debug_log::on_app_start();
    tracing::info!("debug.log 路径：{path_display}");

    tauri::Builder::default()
        // 原生目录/文件选择对话框（设置 → 资源目录「浏览」）
        .plugin(tauri_plugin_dialog::init())
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
        .invoke_handler(tauri::generate_handler![uses_custom_titlebar])
        .run(tauri::generate_context!())
        .expect("error while running DFY 运维平台");
}
