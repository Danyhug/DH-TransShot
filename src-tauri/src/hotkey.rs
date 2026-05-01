use log::info;
use tauri::Emitter;
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

fn emit_hotkey_action(app: tauri::AppHandle, action: &'static str) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(120));
        let _ = app.emit("hotkey-action", action);
    });
}

pub fn setup_hotkeys(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Alt+A: Region screenshot (区域截图)
    let screenshot_shortcut = Shortcut::new(Some(Modifiers::ALT), Code::KeyA);

    // Alt+S: Region OCR + translate (区域翻译)
    let ocr_translate_shortcut = Shortcut::new(Some(Modifiers::ALT), Code::KeyS);

    // Alt+Q: Clipboard translate (翻译选中文本)
    let clipboard_translate_shortcut = Shortcut::new(Some(Modifiers::ALT), Code::KeyQ);

    info!("[Hotkey] 注册快捷键: Alt+A (区域截图), Alt+S (区域翻译), Alt+Q (翻译选中文本)");

    app.global_shortcut().on_shortcuts(
        [screenshot_shortcut, ocr_translate_shortcut, clipboard_translate_shortcut],
        move |app, shortcut, event| {
            if event.state != ShortcutState::Released {
                return;
            }

            if shortcut == &screenshot_shortcut {
                info!("[Hotkey] 触发: Alt+A (screenshot)");
                emit_hotkey_action(app.clone(), "screenshot");
            } else if shortcut == &ocr_translate_shortcut {
                info!("[Hotkey] 触发: Alt+S (ocr_translate)");
                emit_hotkey_action(app.clone(), "ocr_translate");
            } else if shortcut == &clipboard_translate_shortcut {
                info!("[Hotkey] 触发: Alt+Q (clipboard_translate)");
                emit_hotkey_action(app.clone(), "clipboard_translate");
            }
        },
    )?;

    Ok(())
}
