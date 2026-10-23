use crate::config::{AppState, HotkeyConfig};
use log::info;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Emitter, Manager,
};

/// 托盘图标：编译期嵌入，不依赖运行时工作目录。
///
/// 打包成 .app 后进程的工作目录是 `/`，相对路径 `icons/32x32.png` 读不到，以前靠
/// `default_window_icon()` 或 `include_bytes!` 兜底，结果开发时和打包后拿到的可能
/// 不是同一张图。直接嵌进二进制就没有这个问题。
///
/// 两个平台的图案不同：
/// - macOS 用模板图（纯色 + alpha，无底板），系统按菜单栏明暗自动反色
/// - Windows / Linux 没有模板图机制，保留深色圆角底板，保证亮/暗任务栏都看得清
#[cfg(target_os = "macos")]
const TRAY_ICON_PNG: &[u8] = include_bytes!("../icons/tray-macos.png");
#[cfg(not(target_os = "macos"))]
const TRAY_ICON_PNG: &[u8] = include_bytes!("../icons/tray-windows.png");

pub fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let hotkeys = app
        .state::<AppState>()
        .settings
        .lock()
        .map(|settings| settings.hotkeys.clone())
        .unwrap_or_else(|_| HotkeyConfig::default());

    let version_item = MenuItem::with_id(
        app,
        "version",
        format!("DH-TransShot v{}", app.package_info().version),
        false,
        None::<&str>,
    )?;
    let version_separator =
        MenuItem::with_id(app, "sep_version", "─────────", false, None::<&str>)?;

    let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
    let screenshot = MenuItem::with_id(
        app,
        "screenshot",
        format!("区域截图 ({})", hotkeys.screenshot),
        true,
        None::<&str>,
    )?;
    let ocr_translate = MenuItem::with_id(
        app,
        "ocr_translate",
        format!("区域翻译 ({})", hotkeys.ocr_translate),
        true,
        None::<&str>,
    )?;
    let clipboard_translate = MenuItem::with_id(
        app,
        "clipboard_translate",
        format!("翻译选中文本 ({})", hotkeys.clipboard_translate),
        true,
        None::<&str>,
    )?;
    let separator = MenuItem::with_id(app, "sep", "─────────", false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &version_item,
            &version_separator,
            &show,
            &screenshot,
            &ocr_translate,
            &clipboard_translate,
            &separator,
            &quit,
        ],
    )?;

    let tray_builder = TrayIconBuilder::new()
        .icon(Image::from_bytes(TRAY_ICON_PNG)?)
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
            "clipboard_translate" => {
                info!("[Tray] 点击: 翻译选中文本");
                let _ = app.emit("tray-action", "clipboard_translate");
            }
            "quit" => {
                info!("[Tray] 点击: 退出");
                app.exit(0);
            }
            _ => {}
        });

    // macOS 菜单栏图标按模板图渲染，系统会用菜单栏当前的前景色（黑/白）重绘，
    // 这样亮色和深色菜单栏都不用各维护一套图。
    #[cfg(target_os = "macos")]
    let tray_builder = tray_builder.icon_as_template(true);

    let _tray = tray_builder.build(app)?;

    Ok(())
}
