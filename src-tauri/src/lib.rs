mod api_client;
mod commands;
mod config;
mod hotkey;
mod ocr;
mod screenshot;
mod translation;
mod tray;

use config::{AppState, Settings};
use log::{info, warn};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load .env file (silently ignore if not present)
    let _ = dotenvy::dotenv();

    let app_state = AppState::default();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::screenshot::start_region_select,
            commands::screenshot::capture_region,
            commands::screenshot::get_frozen_screenshot,
            commands::ocr::recognize_text,
            commands::translation::translate_text,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::clipboard::read_clipboard,
            commands::clipboard::copy_image_to_clipboard,
        ])
        .setup(|app| {
            info!("[Setup] 应用启动，加载持久化配置...");
            // Load persisted settings from store
            use tauri_plugin_store::StoreExt;
            if let Ok(store) = app.store("settings.json") {
                if let Some(value) = store.get("settings") {
                    if let Ok(settings) = serde_json::from_value::<Settings>(value) {
                        info!("[Setup] 配置加载成功, translation.model={}, ocr.model={}", settings.translation.model, settings.ocr.model);
                        let app_state = app.state::<AppState>();
                        let mut guard = app_state.settings.lock().unwrap();
                        *guard = settings;
                    } else {
                        warn!("[Setup] 配置反序列化失败，使用默认配置");
                    }
                } else {
                    info!("[Setup] 无已保存配置，使用默认配置");
                }
            } else {
                warn!("[Setup] 无法打开 settings.json store");
            }
            tray::setup_tray(app)?;
            info!("[Setup] 系统托盘初始化完成");
            hotkey::setup_hotkeys(app)?;
            info!("[Setup] 全局快捷键注册完成");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
