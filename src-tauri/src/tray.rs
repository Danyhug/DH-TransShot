use log::info;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};

pub fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
    let screenshot = MenuItem::with_id(app, "screenshot", "区域截图 (Alt+A)", true, None::<&str>)?;
    let ocr_translate = MenuItem::with_id(app, "ocr_translate", "区域翻译 (Alt+S)", true, None::<&str>)?;
    let separator = MenuItem::with_id(app, "sep", "─────────", false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[&show, &screenshot, &ocr_translate, &separator, &quit],
    )?;

    let _tray = TrayIconBuilder::new()
        .icon(Image::from_path("icons/32x32.png").unwrap_or_else(|_| {
            app.default_window_icon().cloned().unwrap_or_else(|| {
                Image::from_bytes(include_bytes!("../icons/32x32.png"))
                    .expect("Failed to load tray icon")
            })
        }))
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "show" => {
                info!("[Tray] 点击: 显示窗口");
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "screenshot" => {
                info!("[Tray] 点击: 区域截图");
                let _ = app.emit("tray-action", "screenshot");
            }
            "ocr_translate" => {
                info!("[Tray] 点击: 区域翻译");
                let _ = app.emit("tray-action", "ocr_translate");
            }
            "quit" => {
                info!("[Tray] 点击: 退出");
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}
