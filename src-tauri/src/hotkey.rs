use crate::config::{AppState, HotkeyConfig};
use log::{info, warn};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Global mapping from registered Shortcut to action name (e.g. "screenshot").
/// Populated by setup_hotkeys / reload_hotkeys; consulted by the global handler.
static SHORTCUT_ACTIONS: OnceLock<Mutex<HashMap<Shortcut, String>>> = OnceLock::new();

fn shortcut_actions() -> &'static Mutex<HashMap<Shortcut, String>> {
    SHORTCUT_ACTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn emit_hotkey_action(app: AppHandle, action: String) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(120));
        let _ = app.emit("hotkey-action", action);
    });
}

/// Global shortcut event handler — dispatches based on SHORTCUT_ACTIONS map.
pub fn handle_shortcut_event(
    app: &AppHandle,
    shortcut: &Shortcut,
    event: tauri_plugin_global_shortcut::ShortcutEvent,
) {
    if event.state != ShortcutState::Released {
        return;
    }
    let action = match shortcut_actions().lock() {
        Ok(map) => map.get(shortcut).cloned(),
        Err(e) => {
            warn!("[Hotkey] SHORTCUT_ACTIONS 锁失败: {}", e);
            return;
        }
    };
    if let Some(action) = action {
        info!("[Hotkey] 触发: {:?} -> {}", shortcut, action);
        emit_hotkey_action(app.clone(), action);
    }
}

/// Parse a shortcut string like "Alt+A". Returns Err with reason on failure.
fn parse_shortcut(s: &str) -> Result<Shortcut, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("快捷键为空".to_string());
    }
    Shortcut::from_str(trimmed).map_err(|e| format!("无法解析 '{}': {}", trimmed, e))
}

/// Register the three hotkeys defined in `cfg`. Logs warnings for invalid or
/// duplicate combos but does not abort — partial success is preferable to
/// leaving the user without any hotkeys.
fn apply_hotkeys(app: &AppHandle, cfg: &HotkeyConfig) {
    let entries: [(&str, &str); 3] = [
        ("screenshot", cfg.screenshot.as_str()),
        ("ocr_translate", cfg.ocr_translate.as_str()),
        ("clipboard_translate", cfg.clipboard_translate.as_str()),
    ];

    let mut new_map: HashMap<Shortcut, String> = HashMap::new();
    let mut to_register: Vec<Shortcut> = Vec::new();

    for (action, raw) in entries {
        match parse_shortcut(raw) {
            Ok(sc) => {
                if new_map.contains_key(&sc) {
                    warn!("[Hotkey] 快捷键 '{}' 重复，{} 未生效", raw, action);
                    continue;
                }
                new_map.insert(sc, action.to_string());
                to_register.push(sc);
            }
            Err(e) => warn!("[Hotkey] {} 解析失败: {}", action, e),
        }
    }

    // Swap in the new map first so events arriving during register find it.
    if let Ok(mut map) = shortcut_actions().lock() {
        *map = new_map;
    }

    let gs = app.global_shortcut();
    if let Err(e) = gs.register_multiple(to_register.iter().copied()) {
        warn!("[Hotkey] 注册失败: {}", e);
    } else {
        info!(
            "[Hotkey] 已注册 {} 个快捷键: screenshot={}, ocr_translate={}, clipboard_translate={}",
            to_register.len(),
            cfg.screenshot,
            cfg.ocr_translate,
            cfg.clipboard_translate
        );
    }
}

/// Initial registration on app setup. Reads current settings.
pub fn setup_hotkeys(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let state = handle.state::<AppState>();
    let cfg = {
        let s = state.settings.lock().map_err(|e| e.to_string())?;
        s.hotkeys.clone()
    };
    apply_hotkeys(&handle, &cfg);
    Ok(())
}

/// Re-register hotkeys after settings change. Safe to call from command handlers.
pub fn reload_hotkeys(app: &AppHandle) {
    info!("[Hotkey] 重新加载快捷键...");
    let gs = app.global_shortcut();
    if let Err(e) = gs.unregister_all() {
        warn!("[Hotkey] unregister_all 失败: {}", e);
    }
    let state = app.state::<AppState>();
    let cfg = match state.settings.lock() {
        Ok(s) => s.hotkeys.clone(),
        Err(e) => {
            warn!("[Hotkey] 读取 settings 失败: {}", e);
            return;
        }
    };
    apply_hotkeys(app, &cfg);
}

/// Suspend all global shortcuts. Used while the settings panel is recording
/// new hotkeys — otherwise the OS preempts already-registered combos before
/// they reach the browser keydown listener.
#[tauri::command]
pub async fn suspend_hotkeys(app: AppHandle) -> Result<(), String> {
    info!("[Hotkey] 挂起所有快捷键");
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())
}

/// Re-arm hotkeys from current settings. Called when the settings panel closes.
#[tauri::command]
pub async fn resume_hotkeys(app: AppHandle) -> Result<(), String> {
    reload_hotkeys(&app);
    Ok(())
}
