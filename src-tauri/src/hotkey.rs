use crate::config::{AppState, HotkeyConfig};
use log::{info, warn};
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Set while the settings window records shortcut combinations. Native
/// shortcuts must be unregistered during that period so keydown reaches the
/// webview instead of being consumed by the operating system.
static HOTKEYS_SUSPENDED: AtomicBool = AtomicBool::new(false);

/// Serializes suspend / restore / reload so a settings window that opens and
/// closes quickly cannot leave a late `suspend_hotkeys` call winning the race.
static HOTKEY_OPERATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn hotkey_operation_lock() -> &'static Mutex<()> {
    HOTKEY_OPERATION_LOCK.get_or_init(|| Mutex::new(()))
}

/// Wait for Alt/Option to be released before changing focus or posting input.
/// This avoids carrying the modifier into the screenshot overlay or selected
/// text fallback. Registration itself remains unchanged after the action.
fn dispatch_hotkey_action(app: AppHandle, action: String) {
    if HOTKEYS_SUSPENDED.load(Ordering::Acquire) {
        info!("[Hotkey] 触发跳过：hotkeys 已挂起（设置面板录入中）");
        return;
    }

    std::thread::spawn(move || {
        wait_for_modifier_release();
        let _ = app.emit("hotkey-action", action);
    });
}

/// Block (up to ~500ms) until the Alt/Option modifier is released.
#[cfg(target_os = "macos")]
fn wait_for_modifier_release() {
    // kCGEventFlagMaskAlternate
    const FLAG_ALT: u64 = 1 << 19;
    extern "C" {
        fn CGEventSourceFlagsState(state_id: u32) -> u64;
    }

    for _ in 0..20 {
        let flags = unsafe { CGEventSourceFlagsState(1) };
        if flags & FLAG_ALT == 0 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

#[cfg(not(target_os = "macos"))]
fn wait_for_modifier_release() {
    std::thread::sleep(std::time::Duration::from_millis(120));
}

/// Parse a shortcut string like "Alt+A".
fn parse_shortcut(value: &str) -> Result<Shortcut, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("快捷键为空".to_string());
    }
    Shortcut::from_str(value).map_err(|e| format!("无法解析 '{}': {}", value, e))
}

/// Register each shortcut independently. A conflict in one combination must
/// not prevent the other configured actions from being registered.
fn apply_hotkeys(app: &AppHandle, cfg: &HotkeyConfig) {
    let entries: [(&str, &str); 3] = [
        ("screenshot", cfg.screenshot.as_str()),
        ("ocr_translate", cfg.ocr_translate.as_str()),
        ("clipboard_translate", cfg.clipboard_translate.as_str()),
    ];

    let global_shortcut = app.global_shortcut();
    let mut seen = HashSet::new();
    let mut valid = 0;
    let mut registered = 0;

    for (action, raw) in entries {
        let shortcut = match parse_shortcut(raw) {
            Ok(shortcut) => shortcut,
            Err(e) => {
                warn!("[Hotkey] {} 解析失败: {}", action, e);
                continue;
            }
        };

        if !seen.insert(shortcut) {
            warn!("[Hotkey] 快捷键 '{}' 重复，{} 未生效", raw, action);
            continue;
        }
        valid += 1;

        let action = action.to_string();
        match global_shortcut.on_shortcut(shortcut, move |app, shortcut, event| {
            // Dispatch on Pressed. Waiting for Released is unreliable when an
            // action immediately moves keyboard focus to a new window.
            if event.state != ShortcutState::Pressed {
                return;
            }
            info!("[Hotkey] 触发: {:?} -> {}", shortcut, action);
            dispatch_hotkey_action(app.clone(), action.clone());
        }) {
            Ok(()) => registered += 1,
            Err(e) => warn!(
                "[Hotkey] {:?} 注册失败（可能已被系统或其他应用占用）: {}",
                shortcut, e
            ),
        }
    }

    info!(
        "[Hotkey] 已注册 {}/{} 个快捷键: screenshot={}, ocr_translate={}, clipboard_translate={}",
        registered, valid, cfg.screenshot, cfg.ocr_translate, cfg.clipboard_translate
    );
}

/// Initial registration on application setup.
pub fn setup_hotkeys(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let state = handle.state::<AppState>();
    let cfg = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.hotkeys.clone()
    };
    apply_hotkeys(&handle, &cfg);
    Ok(())
}

/// Re-register hotkeys after settings change. No-op while the settings window
/// is recording a combination.
pub fn reload_hotkeys(app: &AppHandle) {
    let _operation = match hotkey_operation_lock().lock() {
        Ok(guard) => guard,
        Err(e) => {
            warn!("[Hotkey] 操作锁失败: {}", e);
            return;
        }
    };
    reload_hotkeys_locked(app);
}

/// Reload implementation. Caller must hold `HOTKEY_OPERATION_LOCK`.
fn reload_hotkeys_locked(app: &AppHandle) {
    if HOTKEYS_SUSPENDED.load(Ordering::Acquire) {
        info!("[Hotkey] reload 跳过：hotkeys 已挂起（设置面板录入中）");
        return;
    }

    info!("[Hotkey] 重新加载快捷键...");
    if let Err(e) = app.global_shortcut().unregister_all() {
        warn!("[Hotkey] unregister_all 失败: {}", e);
    }

    let state = app.state::<AppState>();
    let cfg = match state.settings.lock() {
        Ok(settings) => settings.hotkeys.clone(),
        Err(e) => {
            warn!("[Hotkey] 读取 settings 失败: {}", e);
            return;
        }
    };
    apply_hotkeys(app, &cfg);
}

/// Suspend native shortcuts while the settings webview records a combination.
#[tauri::command]
pub async fn suspend_hotkeys(app: AppHandle) -> Result<(), String> {
    let _operation = hotkey_operation_lock().lock().map_err(|e| e.to_string())?;

    // The webview can be destroyed before its asynchronous mount invoke reaches
    // Rust. Ignore a late request instead of disabling shortcuts indefinitely.
    if app.get_webview_window("settings").is_none() {
        info!("[Hotkey] 挂起跳过：settings 窗口已不存在");
        return Ok(());
    }

    if HOTKEYS_SUSPENDED.swap(true, Ordering::AcqRel) {
        info!("[Hotkey] 挂起跳过：hotkeys 已处于挂起状态");
        return Ok(());
    }

    info!("[Hotkey] 挂起所有快捷键");
    if let Err(e) = app.global_shortcut().unregister_all() {
        HOTKEYS_SUSPENDED.store(false, Ordering::Release);
        reload_hotkeys_locked(&app);
        return Err(e.to_string());
    }

    // The window may have closed while unregister_all was running.
    if app.get_webview_window("settings").is_none() {
        info!("[Hotkey] settings 在挂起期间关闭，立即恢复快捷键");
        HOTKEYS_SUSPENDED.store(false, Ordering::Release);
        reload_hotkeys_locked(&app);
    }
    Ok(())
}

/// Re-arm shortcuts from current settings. Idempotent so the explicit frontend
/// call, React cleanup and native Destroyed fallback can safely overlap.
pub fn restore_hotkeys(app: &AppHandle) {
    let _operation = match hotkey_operation_lock().lock() {
        Ok(guard) => guard,
        Err(e) => {
            warn!("[Hotkey] 操作锁失败: {}", e);
            return;
        }
    };

    if !HOTKEYS_SUSPENDED.swap(false, Ordering::AcqRel) {
        info!("[Hotkey] 恢复跳过：hotkeys 已处于启用状态");
        return;
    }

    info!("[Hotkey] 恢复快捷键");
    reload_hotkeys_locked(app);
}

#[tauri::command]
pub async fn resume_hotkeys(app: AppHandle) -> Result<(), String> {
    restore_hotkeys(&app);
    Ok(())
}
