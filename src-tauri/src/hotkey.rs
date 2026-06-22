use crate::config::{AppState, HotkeyConfig};
use log::{info, warn};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Global mapping from registered Shortcut to action name (e.g. "screenshot").
/// Populated by setup_hotkeys / reload_hotkeys; consulted by the global handler.
static SHORTCUT_ACTIONS: OnceLock<Mutex<HashMap<Shortcut, String>>> = OnceLock::new();

/// When true, `reload_hotkeys` is a no-op. Set by `suspend_hotkeys` while the
/// settings panel is recording new combinations, and cleared by `resume_hotkeys`.
/// Without this guard, the delayed re-registration scheduled by
/// `emit_hotkey_action` (or by overlay close) could fire during recording and
/// re-arm the OLD shortcuts, blocking the user from capturing the new combo.
static HOTKEYS_SUSPENDED: AtomicBool = AtomicBool::new(false);

/// Guard against duplicate delivery when macOS Carbon and the CGEventTap
/// fallback both observe the same physical key press.
static LAST_HOTKEY_DISPATCH: OnceLock<Mutex<Option<(String, Instant)>>> = OnceLock::new();

const HOTKEY_DEBOUNCE: Duration = Duration::from_millis(500);

fn shortcut_actions() -> &'static Mutex<HashMap<Shortcut, String>> {
    SHORTCUT_ACTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn last_hotkey_dispatch() -> &'static Mutex<Option<(String, Instant)>> {
    LAST_HOTKEY_DISPATCH.get_or_init(|| Mutex::new(None))
}

fn should_dispatch_action(action: &str) -> bool {
    let now = Instant::now();
    let mut last = match last_hotkey_dispatch().lock() {
        Ok(last) => last,
        Err(e) => {
            warn!("[Hotkey] LAST_HOTKEY_DISPATCH 锁失败: {}", e);
            return true;
        }
    };

    if let Some((last_action, last_at)) = last.as_ref() {
        if last_action == action && now.duration_since(*last_at) < HOTKEY_DEBOUNCE {
            info!("[Hotkey] 忽略重复触发: {}", action);
            return false;
        }
    }

    *last = Some((action.to_string(), now));
    true
}

fn dispatch_hotkey_action(app: AppHandle, action: String) {
    if HOTKEYS_SUSPENDED.load(Ordering::Acquire) {
        info!("[Hotkey] 触发跳过：hotkeys 已挂起（设置面板录入中）");
        return;
    }
    if should_dispatch_action(&action) {
        emit_hotkey_action(app, action);
    }
}

/// Wait for Alt to release, emit the action, then schedule a deferred
/// re-registration of all hotkeys.
///
/// Re-registration: macOS Carbon `RegisterEventHotKey` occasionally loses our
/// bindings after the action's OS-level operations (window creation, osascript,
/// CGEvent posting, etc.). Symptom: next press of the hotkey types the literal
/// character (Option+S → ß) instead of firing. We refresh the registration
/// 2s after each hotkey to mask this. `reload_hotkeys` skips silently if the
/// settings panel has hotkeys suspended.
fn emit_hotkey_action(app: AppHandle, action: String) {
    std::thread::spawn(move || {
        wait_for_modifier_release();
        let _ = app.emit("hotkey-action", action);

        // Defer re-registration until the action's OS-disruptive operations
        // have completed. 2s comfortably covers osascript / window creation.
        std::thread::sleep(std::time::Duration::from_secs(2));
        reload_hotkeys(&app);
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
    // Windows/Linux: a short fixed delay is sufficient — global hotkey delivery
    // doesn't get stuck across focus changes the same way macOS Carbon does.
    std::thread::sleep(std::time::Duration::from_millis(120));
}

#[cfg(target_os = "macos")]
mod macos_event_tap {
    #![allow(non_upper_case_globals)]

    use super::{dispatch_hotkey_action, HOTKEYS_SUSPENDED};
    use log::{info, warn};
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::Ordering;
    use std::sync::{mpsc, Arc, Mutex, OnceLock};
    use std::time::Duration;
    use tauri::AppHandle;
    use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut};

    type CGEventRef = *mut c_void;
    type CGEventTapProxy = *mut c_void;
    type CFMachPortRef = *mut c_void;
    type CFRunLoopRef = *mut c_void;
    type CFRunLoopSourceRef = *mut c_void;
    type CFAllocatorRef = *const c_void;
    type CFRunLoopMode = *const c_void;
    type CFIndex = isize;

    const KCG_SESSION_EVENT_TAP: u32 = 1;
    const KCG_HEAD_INSERT_EVENT_TAP: u32 = 0;
    const KCG_EVENT_TAP_OPTION_DEFAULT: u32 = 0;
    const KCG_EVENT_KEY_DOWN: u32 = 10;
    const KCG_EVENT_TAP_DISABLED_BY_TIMEOUT: u32 = 0xFFFF_FFFE;
    const KCG_EVENT_TAP_DISABLED_BY_USER_INPUT: u32 = 0xFFFF_FFFF;
    const KCG_KEYBOARD_EVENT_AUTOREPEAT: u32 = 8;
    const KCG_KEYBOARD_EVENT_KEYCODE: u32 = 9;

    const FLAG_SHIFT: u64 = 1 << 17;
    const FLAG_CONTROL: u64 = 1 << 18;
    const FLAG_ALT: u64 = 1 << 19;
    const FLAG_COMMAND: u64 = 1 << 20;
    const HOTKEY_FLAG_MASK: u64 = FLAG_SHIFT | FLAG_CONTROL | FLAG_ALT | FLAG_COMMAND;

    type CGEventTapCallBack = unsafe extern "C" fn(
        proxy: CGEventTapProxy,
        event_type: u32,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef;

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventTapCreate(
            tap: u32,
            place: u32,
            options: u32,
            events_of_interest: u64,
            callback: CGEventTapCallBack,
            user_info: *mut c_void,
        ) -> CFMachPortRef;
        fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
        fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
        fn CGEventGetFlags(event: CGEventRef) -> u64;
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFAllocatorDefault: CFAllocatorRef;
        static kCFRunLoopCommonModes: CFRunLoopMode;

        fn CFRunLoopGetMain() -> CFRunLoopRef;
        fn CFMachPortCreateRunLoopSource(
            allocator: CFAllocatorRef,
            port: CFMachPortRef,
            order: CFIndex,
        ) -> CFRunLoopSourceRef;
        fn CFRunLoopAddSource(rl: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFRunLoopMode);
        fn CFMachPortInvalidate(port: CFMachPortRef);
        fn CFRelease(cftype: *const c_void);
    }

    #[derive(Clone)]
    struct TapHotkey {
        scan_code: u16,
        flags: u64,
        action: String,
    }

    #[derive(Clone, Copy)]
    struct EventTapHandles {
        tap: CFMachPortRef,
        _source: CFRunLoopSourceRef,
    }

    unsafe impl Send for EventTapHandles {}

    struct TapState {
        app: AppHandle,
        hotkeys: Mutex<Vec<TapHotkey>>,
        handles: Mutex<Option<EventTapHandles>>,
    }

    static TAP_STATE: OnceLock<Arc<TapState>> = OnceLock::new();

    pub(super) fn update_hotkeys(
        app: &AppHandle,
        actions: &'static Mutex<HashMap<Shortcut, String>>,
    ) {
        let state = TAP_STATE
            .get_or_init(|| {
                Arc::new(TapState {
                    app: app.clone(),
                    hotkeys: Mutex::new(Vec::new()),
                    handles: Mutex::new(None),
                })
            })
            .clone();

        let hotkeys = match actions.lock() {
            Ok(map) => map
                .iter()
                .filter_map(|(shortcut, action)| hotkey_from_shortcut(*shortcut, action.clone()))
                .collect::<Vec<_>>(),
            Err(e) => {
                warn!("[Hotkey] event tap 读取快捷键映射失败: {}", e);
                Vec::new()
            }
        };

        if let Ok(mut guard) = state.hotkeys.lock() {
            *guard = hotkeys;
            info!("[Hotkey] macOS event tap 已同步 {} 个快捷键", guard.len());
        }

        ensure_installed(&state);
    }

    fn ensure_installed(state: &Arc<TapState>) {
        if state
            .handles
            .lock()
            .map(|handles| handles.is_some())
            .unwrap_or(false)
        {
            return;
        }

        let state_for_main = state.clone();
        let app = state.app.clone();
        let (tx, rx) = mpsc::channel();
        let schedule_result = app.run_on_main_thread(move || {
            let result = unsafe { create_event_tap(Arc::as_ptr(&state_for_main) as *mut c_void) };

            match result {
                Ok(handles) => {
                    if let Ok(mut guard) = state_for_main.handles.lock() {
                        *guard = Some(handles);
                    }
                    let _ = tx.send(Ok(()));
                }
                Err(e) => {
                    let _ = tx.send(Err(e));
                }
            }
        });

        if let Err(e) = schedule_result {
            warn!("[Hotkey] macOS event tap 安装调度失败: {}", e);
            return;
        }

        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => info!("[Hotkey] macOS event tap 已启用"),
            Ok(Err(e)) => {
                let has_accessibility = unsafe { AXIsProcessTrusted() };
                if has_accessibility {
                    warn!(
                        "[Hotkey] macOS event tap 启用失败: {}，继续使用 Carbon 快捷键",
                        e
                    );
                } else {
                    warn!(
                        "[Hotkey] macOS event tap 启用失败（缺少辅助功能权限）: {}\
                         \n  → 请在 系统设置 > 隐私与安全性 > 辅助功能 中授权 DH-TransShot\
                         \n  → 否则按 Option+Q 等组合键时会输入特殊字符（如 œ）而不是触发快捷键",
                        e
                    );
                }
            }
            Err(e) => warn!("[Hotkey] macOS event tap 启用等待失败: {}", e),
        }
    }

    unsafe fn create_event_tap(user_info: *mut c_void) -> Result<EventTapHandles, String> {
        let events_of_interest = 1_u64 << KCG_EVENT_KEY_DOWN;
        let tap = CGEventTapCreate(
            KCG_SESSION_EVENT_TAP,
            KCG_HEAD_INSERT_EVENT_TAP,
            KCG_EVENT_TAP_OPTION_DEFAULT,
            events_of_interest,
            event_tap_callback,
            user_info,
        );
        if tap.is_null() {
            let has_accessibility = AXIsProcessTrusted();
            if has_accessibility {
                return Err("CGEventTapCreate 返回 null（已有辅助功能权限但仍然失败）".to_string());
            } else {
                return Err("CGEventTapCreate 返回 null，缺少辅助功能权限。\
                     请在 系统设置 > 隐私与安全性 > 辅助功能 中授权 DH-TransShot"
                    .to_string());
            }
        }

        let source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, tap, 0);
        if source.is_null() {
            CFMachPortInvalidate(tap);
            CFRelease(tap as *const c_void);
            return Err("CFMachPortCreateRunLoopSource 返回 null".to_string());
        }

        let run_loop = CFRunLoopGetMain();
        CFRunLoopAddSource(run_loop, source, kCFRunLoopCommonModes);
        CGEventTapEnable(tap, true);

        Ok(EventTapHandles {
            tap,
            _source: source,
        })
    }

    unsafe extern "C" fn event_tap_callback(
        _proxy: CGEventTapProxy,
        event_type: u32,
        event: CGEventRef,
        user_info: *mut c_void,
    ) -> CGEventRef {
        if user_info.is_null() {
            return event;
        }

        let state = &*(user_info as *const TapState);

        if event_type == KCG_EVENT_TAP_DISABLED_BY_TIMEOUT
            || event_type == KCG_EVENT_TAP_DISABLED_BY_USER_INPUT
        {
            if let Ok(handles) = state.handles.lock() {
                if let Some(handles) = *handles {
                    CGEventTapEnable(handles.tap, true);
                }
            }
            return event;
        }

        if event_type != KCG_EVENT_KEY_DOWN || event.is_null() {
            return event;
        }

        if HOTKEYS_SUSPENDED.load(Ordering::Acquire) {
            return event;
        }

        let scan_code = CGEventGetIntegerValueField(event, KCG_KEYBOARD_EVENT_KEYCODE) as u16;
        let flags = CGEventGetFlags(event) & HOTKEY_FLAG_MASK;
        let is_repeat = CGEventGetIntegerValueField(event, KCG_KEYBOARD_EVENT_AUTOREPEAT) != 0;

        let action = match state.hotkeys.lock() {
            Ok(hotkeys) => hotkeys
                .iter()
                .find(|hotkey| hotkey.scan_code == scan_code && hotkey.flags == flags)
                .map(|hotkey| hotkey.action.clone()),
            Err(e) => {
                warn!("[Hotkey] event tap 快捷键映射锁失败: {}", e);
                None
            }
        };

        if let Some(action) = action {
            if !is_repeat {
                info!(
                    "[Hotkey] event tap 捕获: key_code={} -> {}",
                    scan_code, action
                );
                dispatch_hotkey_action(state.app.clone(), action);
            }
            return ptr::null_mut();
        }

        event
    }

    fn hotkey_from_shortcut(shortcut: Shortcut, action: String) -> Option<TapHotkey> {
        Some(TapHotkey {
            scan_code: key_to_scancode(shortcut.key)?,
            flags: mods_to_cg_flags(shortcut.mods),
            action,
        })
    }

    fn mods_to_cg_flags(mods: Modifiers) -> u64 {
        let mut flags = 0;
        if mods.contains(Modifiers::SHIFT) {
            flags |= FLAG_SHIFT;
        }
        if mods.contains(Modifiers::CONTROL) {
            flags |= FLAG_CONTROL;
        }
        if mods.contains(Modifiers::ALT) {
            flags |= FLAG_ALT;
        }
        if mods.intersects(Modifiers::SUPER | Modifiers::META) {
            flags |= FLAG_COMMAND;
        }
        flags
    }

    fn key_to_scancode(code: Code) -> Option<u16> {
        match code {
            Code::KeyA => Some(0x00),
            Code::KeyS => Some(0x01),
            Code::KeyD => Some(0x02),
            Code::KeyF => Some(0x03),
            Code::KeyH => Some(0x04),
            Code::KeyG => Some(0x05),
            Code::KeyZ => Some(0x06),
            Code::KeyX => Some(0x07),
            Code::KeyC => Some(0x08),
            Code::KeyV => Some(0x09),
            Code::KeyB => Some(0x0b),
            Code::KeyQ => Some(0x0c),
            Code::KeyW => Some(0x0d),
            Code::KeyE => Some(0x0e),
            Code::KeyR => Some(0x0f),
            Code::KeyY => Some(0x10),
            Code::KeyT => Some(0x11),
            Code::Digit1 => Some(0x12),
            Code::Digit2 => Some(0x13),
            Code::Digit3 => Some(0x14),
            Code::Digit4 => Some(0x15),
            Code::Digit6 => Some(0x16),
            Code::Digit5 => Some(0x17),
            Code::Equal => Some(0x18),
            Code::Digit9 => Some(0x19),
            Code::Digit7 => Some(0x1a),
            Code::Minus => Some(0x1b),
            Code::Digit8 => Some(0x1c),
            Code::Digit0 => Some(0x1d),
            Code::BracketRight => Some(0x1e),
            Code::KeyO => Some(0x1f),
            Code::KeyU => Some(0x20),
            Code::BracketLeft => Some(0x21),
            Code::KeyI => Some(0x22),
            Code::KeyP => Some(0x23),
            Code::Enter => Some(0x24),
            Code::KeyL => Some(0x25),
            Code::KeyJ => Some(0x26),
            Code::Quote => Some(0x27),
            Code::KeyK => Some(0x28),
            Code::Semicolon => Some(0x29),
            Code::Backslash => Some(0x2a),
            Code::Comma => Some(0x2b),
            Code::Slash => Some(0x2c),
            Code::KeyN => Some(0x2d),
            Code::KeyM => Some(0x2e),
            Code::Period => Some(0x2f),
            Code::Tab => Some(0x30),
            Code::Space => Some(0x31),
            Code::Backquote => Some(0x32),
            Code::Backspace => Some(0x33),
            Code::Escape => Some(0x35),
            Code::F17 => Some(0x40),
            Code::NumpadDecimal => Some(0x41),
            Code::NumpadMultiply => Some(0x43),
            Code::NumpadAdd => Some(0x45),
            Code::NumLock => Some(0x47),
            Code::AudioVolumeUp => Some(0x48),
            Code::AudioVolumeDown => Some(0x49),
            Code::AudioVolumeMute => Some(0x4a),
            Code::NumpadDivide => Some(0x4b),
            Code::NumpadEnter => Some(0x4c),
            Code::NumpadSubtract => Some(0x4e),
            Code::F18 => Some(0x4f),
            Code::F19 => Some(0x50),
            Code::NumpadEqual => Some(0x51),
            Code::Numpad0 => Some(0x52),
            Code::Numpad1 => Some(0x53),
            Code::Numpad2 => Some(0x54),
            Code::Numpad3 => Some(0x55),
            Code::Numpad4 => Some(0x56),
            Code::Numpad5 => Some(0x57),
            Code::Numpad6 => Some(0x58),
            Code::Numpad7 => Some(0x59),
            Code::F20 => Some(0x5a),
            Code::Numpad8 => Some(0x5b),
            Code::Numpad9 => Some(0x5c),
            Code::F5 => Some(0x60),
            Code::F6 => Some(0x61),
            Code::F7 => Some(0x62),
            Code::F3 => Some(0x63),
            Code::F8 => Some(0x64),
            Code::F9 => Some(0x65),
            Code::F11 => Some(0x67),
            Code::F13 => Some(0x69),
            Code::F16 => Some(0x6a),
            Code::F14 => Some(0x6b),
            Code::F10 => Some(0x6d),
            Code::F12 => Some(0x6f),
            Code::F15 => Some(0x71),
            Code::Insert => Some(0x72),
            Code::Home => Some(0x73),
            Code::PageUp => Some(0x74),
            Code::Delete => Some(0x75),
            Code::F4 => Some(0x76),
            Code::End => Some(0x77),
            Code::F2 => Some(0x78),
            Code::PageDown => Some(0x79),
            Code::F1 => Some(0x7a),
            Code::ArrowLeft => Some(0x7b),
            Code::ArrowRight => Some(0x7c),
            Code::ArrowDown => Some(0x7d),
            Code::ArrowUp => Some(0x7e),
            Code::CapsLock => Some(0x39),
            Code::PrintScreen => Some(0x46),
            _ => None,
        }
    }
}

/// Global shortcut event handler — dispatches based on SHORTCUT_ACTIONS map.
///
/// Acts on `Pressed`, NOT `Released`. On macOS, when a hotkey action steals
/// keyboard focus (e.g. creating an overlay window), the user's modifier-key
/// release can be delivered to the new front app instead of Carbon's hotkey
/// system, leaving subsequent presses silently undelivered. Acting on Pressed
/// avoids that class of failure; modifier-still-held side effects are handled
/// inside `emit_hotkey_action` by waiting for the modifier to release before
/// emitting.
pub fn handle_shortcut_event(
    app: &AppHandle,
    shortcut: &Shortcut,
    event: tauri_plugin_global_shortcut::ShortcutEvent,
) {
    if event.state != ShortcutState::Pressed {
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
        dispatch_hotkey_action(app.clone(), action);
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

    #[cfg(target_os = "macos")]
    macos_event_tap::update_hotkeys(app, shortcut_actions());

    let gs = app.global_shortcut();
    // Use on_shortcuts (per-shortcut handler stored in plugin) instead of
    // register_multiple + with_handler (global handler). Both code paths walk
    // the same `shortcuts_.lock().get(&e.id)` lookup inside the plugin, but in
    // practice the per-shortcut path proved more reliable — switching to the
    // global-handler architecture in commit d8a9e23 introduced occasional
    // dropped events on macOS that the per-shortcut path didn't have.
    if let Err(e) = gs.on_shortcuts(to_register.iter().copied(), handle_shortcut_event) {
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
///
/// No-op while `HOTKEYS_SUSPENDED` is set — see that static's doc comment for
/// why this matters. Callers that need an unconditional reload should clear
/// the flag first (as `resume_hotkeys` does).
pub fn reload_hotkeys(app: &AppHandle) {
    if HOTKEYS_SUSPENDED.load(Ordering::Acquire) {
        info!("[Hotkey] reload 跳过：hotkeys 已挂起（设置面板录入中）");
        return;
    }
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
    HOTKEYS_SUSPENDED.store(true, Ordering::Release);
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())
}

/// Re-arm hotkeys from current settings. Called when the settings panel closes.
#[tauri::command]
pub async fn resume_hotkeys(app: AppHandle) -> Result<(), String> {
    HOTKEYS_SUSPENDED.store(false, Ordering::Release);
    reload_hotkeys(&app);
    Ok(())
}
