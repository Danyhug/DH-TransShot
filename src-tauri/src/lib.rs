mod api_client;
mod commands;
mod config;
mod hotkey;
mod ocr;
mod screenshot;
mod translation;
mod tray;
mod tts;

use config::{AppState, Settings};
use log::{info, warn};
use tauri::{Listener, Manager, RunEvent, WindowEvent};
use tauri_plugin_log::{Target, TargetKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load .env file (silently ignore if not present)
    let _ = dotenvy::dotenv();

    let app_state = AppState::default();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir { file_name: None }),
                ])
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new()
            .with_handler(hotkey::handle_shortcut_event)
            .build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            commands::screenshot::start_region_select,
            commands::screenshot::capture_region,
            commands::screenshot::get_frozen_screenshot,
            commands::ocr::capture_and_ocr,
            commands::translation::translate_text,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::clipboard::read_clipboard,
            commands::clipboard::copy_image_to_clipboard,
            commands::clipboard::read_selected_text,
            commands::clipboard::save_file,
            commands::tts::synthesize_speech,
            hotkey::suspend_hotkeys,
            hotkey::resume_hotkeys,
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

            // Register overlay close listeners once at app level (not per screenshot session)
            let app_handle = app.handle().clone();
            app.listen("close-all-overlays", move |_| {
                info!("[Setup] 收到 close-all-overlays 事件");
                commands::screenshot::close_all_overlays(&app_handle);
            });
            let app_handle = app.handle().clone();
            app.listen("close-other-overlays", move |event| {
                let keep: String = serde_json::from_str(event.payload()).unwrap_or_default();
                info!("[Setup] 收到 close-other-overlays 事件, keep={}", keep);
                commands::screenshot::close_other_overlays(&app_handle, &keep);
            });

            // 拦截主窗口关闭事件，改为隐藏（macOS + Windows 统一行为）
            if let Some(window) = app.get_webview_window("main") {
                let win = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        info!("[Setup] 主窗口关闭请求，拦截并隐藏");
                        api.prevent_close();
                        let _ = win.hide();
                    }
                });
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // macOS: 点击 Dock 图标时显示主窗口
            #[cfg(target_os = "macos")]
            if let RunEvent::Reopen { .. } = event {
                info!("[App] Dock 图标点击，显示主窗口");
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            // 避免未使用变量警告
            #[cfg(not(target_os = "macos"))]
            let _ = app_handle;
        });
}
