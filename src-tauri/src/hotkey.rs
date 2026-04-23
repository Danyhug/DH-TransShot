use log::info;
use tauri::Emitter;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut};

pub fn setup_hotkeys(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Alt+A: Region screenshot (区域截图)
    let screenshot_shortcut = Shortcut::new(Some(Modifiers::ALT), Code::KeyA);

    // Alt+S: Region OCR + translate (区域翻译)
    let ocr_translate_shortcut = Shortcut::new(Some(Modifiers::ALT), Code::KeyS);

    // Alt+D: Clipboard translate (翻译选中文本)
    let clipboard_translate_shortcut = Shortcut::new(Some(Modifiers::ALT), Code::KeyD);

    info!("[Hotkey] 注册快捷键: Alt+A (区域截图), Alt+S (区域翻译), Alt+D (翻译选中文本)");

    app.global_shortcut().on_shortcuts(
        [screenshot_shortcut, ocr_translate_shortcut, clipboard_translate_shortcut],
        move |app, shortcut, _event| {
            if shortcut == &screenshot_shortcut {
                info!("[Hotkey] 触发: Alt+A (screenshot)");
                let _ = app.emit("hotkey-action", "screenshot");
            } else if shortcut == &ocr_translate_shortcut {
                info!("[Hotkey] 触发: Alt+S (ocr_translate)");
                let _ = app.emit("hotkey-action", "ocr_translate");
            } else if shortcut == &clipboard_translate_shortcut {
                info!("[Hotkey] 触发: Alt+D (clipboard_translate)");
                let _ = app.emit("hotkey-action", "clipboard_translate");
            }
        },
    )?;

    Ok(())
}
