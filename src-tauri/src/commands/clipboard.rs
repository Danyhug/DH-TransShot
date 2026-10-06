use log::{error, info, warn};

/// Helper: build a PowerShell Command with CREATE_NO_WINDOW on Windows to suppress the console window.
#[cfg(target_os = "windows")]
fn powershell_command(script: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut cmd = std::process::Command::new("powershell");
    cmd.args(["-command", script]);
    cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    cmd
}

/// Helper: run pbpaste with UTF-8 locale to ensure emoji and multi-byte chars are decoded correctly.
#[cfg(target_os = "macos")]
fn pbpaste_utf8() -> std::process::Command {
    let mut cmd = std::process::Command::new("pbpaste");
    cmd.env("LC_CTYPE", "UTF-8");
    cmd
}

#[cfg(target_os = "macos")]
fn wait_for_option_key_release() -> bool {
    const KCG_EVENT_FLAG_MASK_ALTERNATE: u64 = 1 << 19;

    extern "C" {
        fn CGEventSourceFlagsState(state_id: u32) -> u64;
    }

    for _ in 0..20 {
        let flags = unsafe { CGEventSourceFlagsState(1) };
        if flags & KCG_EVENT_FLAG_MASK_ALTERNATE == 0 {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }

    warn!("[Clipboard] Option/Alt 仍处于按下状态，仍继续（CGEvent 会强制干净的 Cmd+C）");
    false
}

/// Post a synthetic Cmd+C via CGEvent with EXPLICIT modifier flags.
///
/// Why not osascript `keystroke "c" using command down`? AppleScript's keystroke
/// merges its requested modifiers with any modifier keys the user is currently
/// holding. When Alt+Q triggers our hotkey and we fall back to clipboard copy,
/// the user's Alt may still be physically/logically pressed for a brief window
/// — the synthesized Cmd+C then arrives as Cmd+Option+C, which Chrome reads as
/// "Inspect Element" and pops the dev tools.
///
/// CGEventSetFlags overrides the event's modifier flags regardless of hardware
/// state, so the target app receives a clean Cmd+C every time.
#[cfg(target_os = "macos")]
fn simulate_cmd_c_via_cgevent() -> Result<(), String> {
    type CGEventRef = *mut std::ffi::c_void;
    type CGEventSourceRef = *mut std::ffi::c_void;

    extern "C" {
        fn CGEventSourceCreate(state_id: i32) -> CGEventSourceRef;
        fn CGEventCreateKeyboardEvent(
            source: CGEventSourceRef,
            virtual_key: u16,
            key_down: bool,
        ) -> CGEventRef;
        fn CGEventSetFlags(event: CGEventRef, flags: u64);
        fn CGEventPost(tap: u32, event: CGEventRef);
        fn CFRelease(cf: *const std::ffi::c_void);
    }

    const KEY_C: u16 = 8;
    const FLAG_COMMAND: u64 = 1 << 20; // kCGEventFlagMaskCommand
    const HID_EVENT_TAP: u32 = 0; // kCGHIDEventTap
    const COMBINED_SESSION_STATE: i32 = 0; // kCGEventSourceStateCombinedSessionState

    unsafe {
        let source = CGEventSourceCreate(COMBINED_SESSION_STATE);
        if source.is_null() {
            return Err("CGEventSourceCreate 失败".to_string());
        }

        let key_down = CGEventCreateKeyboardEvent(source, KEY_C, true);
        let key_up = CGEventCreateKeyboardEvent(source, KEY_C, false);
        if key_down.is_null() || key_up.is_null() {
            if !key_down.is_null() {
                CFRelease(key_down);
            }
            if !key_up.is_null() {
                CFRelease(key_up);
            }
            CFRelease(source);
            return Err("CGEventCreateKeyboardEvent 失败".to_string());
        }

        // Force flags = Cmd only — clears Alt/Shift/Ctrl that the user may still hold.
        CGEventSetFlags(key_down, FLAG_COMMAND);
        CGEventSetFlags(key_up, FLAG_COMMAND);

        CGEventPost(HID_EVENT_TAP, key_down);
        std::thread::sleep(std::time::Duration::from_millis(10));
        CGEventPost(HID_EVENT_TAP, key_up);

        CFRelease(key_down);
        CFRelease(key_up);
        CFRelease(source);
    }

    Ok(())
}

/// Read `NSPasteboard.generalPasteboard.changeCount` via the Objective-C
/// runtime. macOS increments this monotonic counter on every real clipboard
/// write, so comparing it before/after a synthetic Cmd+C reliably detects
/// whether the copy actually landed — without a fixed sleep or content-equality
/// guessing (which fails when the selection equals the previous clipboard).
///
/// Returns -1 if the runtime lookup fails (treated as "unavailable").
#[cfg(target_os = "macos")]
fn pasteboard_change_count() -> i64 {
    use std::ffi::c_void;
    use std::os::raw::c_char;

    // Force AppKit to be linked so NSPasteboard is registered with the runtime.
    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    extern "C" {
        fn objc_getClass(name: *const c_char) -> *mut c_void;
        fn sel_registerName(name: *const c_char) -> *mut c_void;
        fn objc_msgSend();
    }

    unsafe {
        let cls = objc_getClass(b"NSPasteboard\0".as_ptr() as *const c_char);
        if cls.is_null() {
            return -1;
        }

        // objc_msgSend must be invoked through a prototype matching the method
        // signature (required on arm64). Coerce the imported symbol to a fn
        // pointer, then transmute it to each concrete signature we need.
        let msg_send = objc_msgSend as unsafe extern "C" fn();

        let general_pasteboard: extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void =
            std::mem::transmute(msg_send);
        let pasteboard = general_pasteboard(
            cls,
            sel_registerName(b"generalPasteboard\0".as_ptr() as *const c_char),
        );
        if pasteboard.is_null() {
            return -1;
        }

        let change_count: extern "C" fn(*mut c_void, *mut c_void) -> i64 =
            std::mem::transmute(msg_send);
        change_count(
            pasteboard,
            sel_registerName(b"changeCount\0".as_ptr() as *const c_char),
        )
    }
}

/// 缺少辅助功能权限时返回给前端的提示（macOS 27 起该权限在设置里改名为「设备控制和数据访问」）。
#[cfg(target_os = "macos")]
const AX_PERMISSION_HINT: &str = "缺少辅助功能权限，无法读取选中文字。请在「系统设置 → 隐私与安全性 → 设备控制和数据访问」（macOS 26 及更早为「辅助功能」）中允许 DH-TransShot 后重试；若列表里已有旧条目，先删除再重新添加。";

/// Read selected text from the currently focused application.
/// macOS: 先确认辅助功能权限（未授权时触发系统授权弹窗并返回明确错误），
/// 再用原生 AXUIElement 读取 AXSelectedText，读不到时回退到剪贴板模拟。
/// Windows: 直接走剪贴板模拟（原生 SendInput + 剪贴板序列号，见 `win_input`）。
#[tauri::command]
pub async fn read_selected_text() -> Result<String, String> {
    info!("[Clipboard] read_selected_text: 读取选中文本...");

    let result = tokio::task::spawn_blocking(|| {
        #[cfg(target_os = "macos")]
        {
            // 两条路径（AX 读取、CGEventPost 模拟 Cmd+C）都依赖该权限，缺权限时直接报错，
            // 否则回退路径会返回空串，前端表现为按了快捷键毫无反应
            if !ax::is_trusted(true) {
                warn!("[Clipboard] 未获得辅助功能权限，已请求系统授权");
                return Err(AX_PERMISSION_HINT.to_string());
            }

            match ax::focused_selected_text() {
                Ok(text) if !text.is_empty() => {
                    info!(
                        "[Clipboard] Accessibility API 获取成功, 文本长度={}",
                        text.len()
                    );
                    return Ok(text);
                }
                Ok(_) => info!("[Clipboard] Accessibility API 返回空，尝试剪贴板回退..."),
                Err(e) => info!(
                    "[Clipboard] Accessibility API 失败 ({})，尝试剪贴板回退...",
                    e
                ),
            }
        }

        // Fallback: clipboard simulation with restore
        get_selected_text_clipboard_fallback()
    })
    .await
    .map_err(|e| e.to_string())?;

    result
}

/// 原生 Accessibility API（ApplicationServices + CoreFoundation 裸 FFI）。
///
/// 以前用 `osascript` 驱动 System Events 读 AXSelectedText，额外依赖「自动操作」权限，
/// 且授权常被记到 osascript/终端而不是本应用；应用自己也从未申请过辅助功能权限，
/// 导致 macOS 27 上设置列表里根本找不到 DH-TransShot。直接调用 AX API 后权限归属到本应用。
#[cfg(target_os = "macos")]
mod ax {
    use std::ffi::{c_void, CString};
    use std::os::raw::c_char;

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type AXUIElementRef = *const c_void;

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_AX_ERROR_SUCCESS: i32 = 0;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFTypeDictionaryKeyCallBacks: [u8; 0];
        static kCFTypeDictionaryValueCallBacks: [u8; 0];
        static kCFBooleanTrue: CFTypeRef;
        static kCFBooleanFalse: CFTypeRef;
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFDictionaryCreate(
            alloc: *const c_void,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            num_values: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> CFDictionaryRef;
        fn CFGetTypeID(cf: CFTypeRef) -> usize;
        fn CFStringGetTypeID() -> usize;
        fn CFStringGetLength(s: CFStringRef) -> isize;
        fn CFStringGetMaximumSizeForEncoding(length: isize, encoding: u32) -> isize;
        fn CFStringGetCString(
            s: CFStringRef,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> bool;
        fn CFRelease(cf: CFTypeRef);
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: CFStringRef;
        fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: f32) -> i32;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> i32;
    }

    /// 持有一个 CF 对象，离开作用域自动 CFRelease。
    struct Owned(CFTypeRef);

    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CFRelease(self.0) };
            }
        }
    }

    fn cf_string(s: &str) -> Owned {
        let c = CString::new(s).expect("CF string literal contains NUL");
        Owned(unsafe {
            CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING_UTF8)
        })
    }

    /// CFString → Rust String；非 CFString 或转换失败返回 None。
    fn to_string(cf: CFTypeRef) -> Option<String> {
        unsafe {
            if cf.is_null() || CFGetTypeID(cf) != CFStringGetTypeID() {
                return None;
            }
            let len = CFStringGetLength(cf);
            let cap = CFStringGetMaximumSizeForEncoding(len, K_CF_STRING_ENCODING_UTF8) + 1;
            let mut buf = vec![0u8; cap.max(1) as usize];
            if !CFStringGetCString(
                cf,
                buf.as_mut_ptr() as *mut c_char,
                cap,
                K_CF_STRING_ENCODING_UTF8,
            ) {
                return None;
            }
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            buf.truncate(end);
            String::from_utf8(buf).ok()
        }
    }

    /// 当前进程是否已获辅助功能授权。`prompt = true` 时，未授权会把应用登记进设置列表
    /// 并弹出系统授权框（系统只会弹一次，之后需用户去设置里手动打开）。
    pub fn is_trusted(prompt: bool) -> bool {
        unsafe {
            let keys = [kAXTrustedCheckOptionPrompt];
            let values = [if prompt {
                kCFBooleanTrue
            } else {
                kCFBooleanFalse
            }];
            let options = Owned(CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                kCFTypeDictionaryKeyCallBacks.as_ptr() as *const c_void,
                kCFTypeDictionaryValueCallBacks.as_ptr() as *const c_void,
            ));
            AXIsProcessTrustedWithOptions(options.0)
        }
    }

    fn copy_attribute(element: AXUIElementRef, name: &str) -> Result<Owned, String> {
        let attr = cf_string(name);
        let mut value: CFTypeRef = std::ptr::null();
        let err = unsafe { AXUIElementCopyAttributeValue(element, attr.0, &mut value) };
        if err != K_AX_ERROR_SUCCESS || value.is_null() {
            return Err(format!("读取 {} 失败, AXError={}", name, err));
        }
        Ok(Owned(value))
    }

    /// 读取当前焦点控件的选中文字。
    pub fn focused_selected_text() -> Result<String, String> {
        let system = Owned(unsafe { AXUIElementCreateSystemWide() });
        if system.0.is_null() {
            return Err("AXUIElementCreateSystemWide 失败".to_string());
        }
        // 默认 6 秒；前台应用卡死时别让快捷键跟着卡住，超时后走剪贴板回退
        unsafe { AXUIElementSetMessagingTimeout(system.0, 1.0) };

        let focused = copy_attribute(system.0, "AXFocusedUIElement")?;
        let selected = copy_attribute(focused.0, "AXSelectedText")?;
        to_string(selected.0).ok_or_else(|| "AXSelectedText 不是字符串".to_string())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn cf_string_round_trips_utf8() {
            for s in ["", "KPI", "你好，世界 👋"] {
                let cf = cf_string(s);
                assert_eq!(to_string(cf.0).as_deref(), Some(s));
            }
        }

        #[test]
        fn non_string_is_rejected() {
            assert_eq!(to_string(std::ptr::null()), None);
            assert_eq!(to_string(unsafe { kCFBooleanTrue }), None);
        }

        /// 只查询不弹窗，确认 FFI 链接与调用本身可用（结果取决于运行环境的授权状态）
        #[test]
        fn trust_query_does_not_crash() {
            let _ = is_trusted(false);
        }
    }
}

/// Fallback: simulate Cmd/Ctrl+C, wait for the copy to actually land, read the
/// selection, then restore the previous clipboard content. Dispatches to a
/// platform-specific implementation because the "did the copy land" signal
/// differs (macOS: pasteboard changeCount; Windows: content change).
fn get_selected_text_clipboard_fallback() -> Result<String, String> {
    info!("[Clipboard] 剪贴板回退: 保存剪贴板 → 模拟复制 → 等待更新 → 读取 → 恢复");

    #[cfg(target_os = "macos")]
    {
        get_selected_text_clipboard_fallback_macos()
    }

    #[cfg(target_os = "windows")]
    {
        get_selected_text_clipboard_fallback_windows()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("Clipboard not supported on this platform".to_string())
    }
}

/// macOS clipboard fallback driven by `NSPasteboard.changeCount`. Polling the
/// counter (instead of a fixed sleep) tolerates slow apps and correctly reports
/// "no selection" even when the selection equals the previous clipboard.
#[cfg(target_os = "macos")]
fn get_selected_text_clipboard_fallback_macos() -> Result<String, String> {
    // Save the current clipboard so it can be restored afterwards.
    let saved_clipboard: Option<String> = pbpaste_utf8()
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned());

    // Best-effort: prefer to fire once Option is released. Not fatal —
    // simulate_cmd_c_via_cgevent forces clean Cmd-only flags regardless.
    wait_for_option_key_release();

    // Snapshot the change counter BEFORE copying so a real write is detectable
    // even when the selection is identical to the current clipboard content.
    let count_before = pasteboard_change_count();
    if count_before < 0 {
        warn!("[Clipboard] 无法读取 NSPasteboard changeCount");
    }

    if let Err(e) = simulate_cmd_c_via_cgevent() {
        warn!("[Clipboard] CGEvent 模拟 Cmd+C 失败: {}", e);
    }

    // Poll up to ~1s; stop the instant the pasteboard actually changes. This
    // replaces a fixed 150ms sleep that raced against slow-responding apps.
    let mut copied = false;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(25));
        if pasteboard_change_count() != count_before {
            copied = true;
            break;
        }
    }

    if !copied {
        info!("[Clipboard] changeCount 未变化，可能没有选中文本");
        return Ok(String::new());
    }

    // The selection now sits on the clipboard — read it before restoring.
    let new_text = pbpaste_utf8()
        .output()
        .map_err(|e| e.to_string())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())?;

    // Restore the previous clipboard content (best effort).
    if let Some(ref old_text) = saved_clipboard {
        restore_clipboard_macos(old_text);
        info!("[Clipboard] 原剪贴板内容已恢复");
    }

    info!(
        "[Clipboard] 选中文字已获取 (剪贴板回退), 文本长度={}",
        new_text.len()
    );
    Ok(new_text)
}

/// Best-effort restore of macOS clipboard text via `pbcopy`.
#[cfg(target_os = "macos")]
fn restore_clipboard_macos(text: &str) {
    let _ = std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(stdin) = child.stdin.as_mut() {
                let _ = stdin.write_all(text.as_bytes());
            }
            child.wait()
        });
}

/// Windows clipboard fallback driven by `GetClipboardSequenceNumber`, the
/// counterpart of macOS changeCount: it increments on every clipboard write, so
/// a copy is detected even when the selection equals the previous content.
///
/// Previously this shelled out to PowerShell (`Get-Clipboard` + `SendKeys`):
/// each call took hundreds of ms, SendKeys merged the still-held Alt into
/// Ctrl+Alt+C, and stdout came back in the OEM code page so non-ASCII text was
/// garbled — and then written back over the user's clipboard.
#[cfg(target_os = "windows")]
fn get_selected_text_clipboard_fallback_windows() -> Result<String, String> {
    let saved_clipboard = match win_clipboard::read_text() {
        Ok(text) => text,
        Err(e) => {
            warn!("[Clipboard] 读取原剪贴板失败，结束后不恢复: {}", e);
            None
        }
    };

    if !crate::win_input::wait_for_modifiers_release(500) {
        warn!("[Clipboard] Alt/Shift/Win 仍处于按下状态，仍继续（会先注入 key-up）");
    }

    let seq_before = win_clipboard::sequence_number();
    if let Err(e) = crate::win_input::send_ctrl_c() {
        warn!("[Clipboard] 模拟 Ctrl+C 失败: {}", e);
    }

    // Poll up to ~1s; stop the instant the clipboard actually changes.
    let mut copied = false;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(25));
        if win_clipboard::sequence_number() != seq_before {
            copied = true;
            break;
        }
    }

    if !copied {
        info!("[Clipboard] 剪贴板序列号未变化，可能没有选中文本");
        return Ok(String::new());
    }

    let new_text = win_clipboard::read_text()?.unwrap_or_default();

    // Restore the previous clipboard content (best effort).
    if let Some(ref old_text) = saved_clipboard {
        match win_clipboard::write_text(old_text) {
            Ok(()) => info!("[Clipboard] 原剪贴板内容已恢复"),
            Err(e) => warn!("[Clipboard] 恢复原剪贴板失败: {}", e),
        }
    }

    info!(
        "[Clipboard] 选中文字已获取 (剪贴板回退), 文本长度={}",
        new_text.len()
    );
    Ok(new_text)
}

/// 原生 Win32 剪贴板读写（CF_UNICODETEXT）。
#[cfg(target_os = "windows")]
mod win_clipboard {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
        IsClipboardFormatAvailable, OpenClipboard, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
    };
    use windows_sys::Win32::System::Ole::CF_UNICODETEXT;

    pub fn sequence_number() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    /// 打开期间独占剪贴板，离开作用域自动 CloseClipboard。
    struct Opened;

    impl Opened {
        /// 刚执行复制的应用可能还占着剪贴板，短暂重试。
        fn open() -> Result<Self, String> {
            for _ in 0..10 {
                if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
                    return Ok(Opened);
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err("OpenClipboard 失败（剪贴板被其他程序占用）".to_string())
        }
    }

    impl Drop for Opened {
        fn drop(&mut self) {
            unsafe { CloseClipboard() };
        }
    }

    /// 读取剪贴板文本；剪贴板里没有文本（空或只有图片等）时返回 None。
    pub fn read_text() -> Result<Option<String>, String> {
        let _clipboard = Opened::open()?;
        unsafe {
            if IsClipboardFormatAvailable(CF_UNICODETEXT as u32) == 0 {
                return Ok(None);
            }
            let handle = GetClipboardData(CF_UNICODETEXT as u32);
            if handle.is_null() {
                return Err("GetClipboardData 失败".to_string());
            }
            let ptr = GlobalLock(handle) as *const u16;
            if ptr.is_null() {
                return Err("GlobalLock 失败".to_string());
            }
            // 以 NUL 结尾，但不信任它一定存在，用块大小兜底
            let max_len = GlobalSize(handle) / 2;
            let wide = std::slice::from_raw_parts(ptr, max_len);
            let len = wide.iter().position(|&c| c == 0).unwrap_or(max_len);
            let text = String::from_utf16_lossy(&wide[..len]);
            GlobalUnlock(handle);
            Ok(Some(text))
        }
    }

    pub fn write_text(text: &str) -> Result<(), String> {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let _clipboard = Opened::open()?;
        unsafe {
            if EmptyClipboard() == 0 {
                return Err("EmptyClipboard 失败".to_string());
            }
            let handle = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
            if handle.is_null() {
                return Err("GlobalAlloc 失败".to_string());
            }
            let ptr = GlobalLock(handle) as *mut u16;
            if ptr.is_null() {
                GlobalFree(handle);
                return Err("GlobalLock 失败".to_string());
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            GlobalUnlock(handle);
            // 成功后内存归系统所有，失败时才需要自己释放
            if SetClipboardData(CF_UNICODETEXT as u32, handle).is_null() {
                GlobalFree(handle);
                return Err("SetClipboardData 失败".to_string());
            }
        }
        Ok(())
    }
}

/// Read text from the system clipboard.
#[tauri::command]
pub async fn read_clipboard() -> Result<String, String> {
    info!("[Clipboard] read_clipboard 请求");
    let result = tokio::task::spawn_blocking(|| {
        #[cfg(target_os = "macos")]
        {
            pbpaste_utf8()
                .output()
                .map_err(|e| e.to_string())
                .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        }
        #[cfg(target_os = "windows")]
        {
            win_clipboard::read_text().map(Option::unwrap_or_default)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            Err("Clipboard not supported on this platform".to_string())
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    match &result {
        Ok(text) => info!("[Clipboard] 读取成功, 长度={}", text.len()),
        Err(e) => error!("[Clipboard] 读取失败: {}", e),
    }
    result
}

/// Save base64 PNG data to a file path chosen by the user.
#[tauri::command]
pub async fn save_file(path: String, base64_data: String) -> Result<(), String> {
    info!(
        "[Clipboard] save_file, path={}, base64 size={}",
        path,
        base64_data.len()
    );
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&base64_data)
        .map_err(|e| format!("base64 decode failed: {}", e))?;
    std::fs::write(&path, &bytes).map_err(|e| format!("write file failed: {}", e))?;
    info!("[Clipboard] 文件已保存: {}, size={}", path, bytes.len());
    Ok(())
}

/// Copy an image (base64 PNG) to the system clipboard.
#[tauri::command]
pub async fn copy_image_to_clipboard(image_base64: String) -> Result<(), String> {
    info!(
        "[Clipboard] copy_image_to_clipboard, base64 size={}",
        image_base64.len()
    );
    let result = tokio::task::spawn_blocking(move || {
        use base64::Engine;

        // Decode base64 to PNG bytes
        let png_bytes = base64::engine::general_purpose::STANDARD
            .decode(&image_base64)
            .map_err(|e| format!("base64 decode failed: {}", e))?;

        // Write to temp file
        let tmp_path = std::env::temp_dir().join("dh_transshot_clipboard.png");
        std::fs::write(&tmp_path, &png_bytes)
            .map_err(|e| format!("Failed to write temp file: {}", e))?;

        info!(
            "[Clipboard] 临时文件已写入: {:?}, size={}",
            tmp_path,
            png_bytes.len()
        );

        #[cfg(target_os = "macos")]
        {
            let script = format!(
                "set the clipboard to (read (POSIX file \"{}\") as «class PNGf»)",
                tmp_path.display()
            );
            let output = std::process::Command::new("osascript")
                .args(["-e", &script])
                .output()
                .map_err(|e| format!("osascript failed: {}", e))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let _ = std::fs::remove_file(&tmp_path);
                return Err(format!("osascript error: {}", stderr));
            }
        }

        #[cfg(target_os = "windows")]
        {
            let ps_script = format!(
                "Add-Type -AssemblyName System.Windows.Forms; \
                 [System.Windows.Forms.Clipboard]::SetImage(\
                 [System.Drawing.Image]::FromFile('{}'))",
                tmp_path.display()
            );
            let output = powershell_command(&ps_script)
                .output()
                .map_err(|e| format!("powershell failed: {}", e))?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let _ = std::fs::remove_file(&tmp_path);
                return Err(format!("powershell error: {}", stderr));
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = std::fs::remove_file(&tmp_path);
            return Err("Clipboard not supported on this platform".to_string());
        }

        // Clean up temp file
        let _ = std::fs::remove_file(&tmp_path);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;

    match &result {
        Ok(()) => info!("[Clipboard] 图片已复制到剪贴板"),
        Err(e) => error!("[Clipboard] 图片复制失败: {}", e),
    }
    result
}
