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

/// Read selected text from the currently focused application.
/// Primary: macOS Accessibility API (AXSelectedText).
/// Fallback: save clipboard → simulate Cmd/Ctrl+C → read → restore clipboard.
#[tauri::command]
pub async fn read_selected_text() -> Result<String, String> {
    info!("[Clipboard] read_selected_text: 读取选中文本...");

    let result = tokio::task::spawn_blocking(|| {
        // Try Accessibility API first (macOS only)
        #[cfg(target_os = "macos")]
        {
            if let Ok(text) = get_selected_text_accessibility() {
                if !text.is_empty() {
                    info!(
                        "[Clipboard] Accessibility API 获取成功, 文本长度={}",
                        text.len()
                    );
                    return Ok(text);
                }
                info!("[Clipboard] Accessibility API 返回空，尝试剪贴板回退...");
            } else {
                info!("[Clipboard] Accessibility API 失败，尝试剪贴板回退...");
            }
        }

        // Fallback: clipboard simulation with restore
        get_selected_text_clipboard_fallback()
    })
    .await
    .map_err(|e| e.to_string())?;

    result
}

/// Use macOS Accessibility API to read the selected text directly.
#[cfg(target_os = "macos")]
fn get_selected_text_accessibility() -> Result<String, String> {
    let script = r#"
tell application "System Events"
    set frontApp to name of first application process whose frontmost is true
    tell process frontApp
        try
            set selectedText to value of attribute "AXSelectedText" of focused UI element
            return selectedText
        end try
    end tell
end tell
return ""
"#;

    let output = std::process::Command::new("osascript")
        .args(["-e", script])
        .output()
        .map_err(|e| format!("osascript failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Accessibility API error: {}", stderr));
    }

    let text = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string();

    Ok(text)
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

/// Windows clipboard fallback: poll `Get-Clipboard` until the content changes,
/// returning early instead of relying on a single fixed wait.
#[cfg(target_os = "windows")]
fn get_selected_text_clipboard_fallback_windows() -> Result<String, String> {
    let saved_clipboard: Option<String> = powershell_command("Get-Clipboard")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned());

    let copy_result = powershell_command(
        "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait(\"^c\")",
    )
    .output()
    .map_err(|e| format!("powershell failed: {}", e))?;
    if !copy_result.status.success() {
        let stderr = String::from_utf8_lossy(&copy_result.stderr);
        warn!("[Clipboard] 模拟复制失败: {}", stderr);
    }

    let saved_trimmed = saved_clipboard.as_deref().unwrap_or("").trim().to_string();

    // Poll up to ~720ms; return as soon as the clipboard differs from before.
    let mut new_text = String::new();
    for _ in 0..6 {
        std::thread::sleep(std::time::Duration::from_millis(120));
        let current = powershell_command("Get-Clipboard")
            .output()
            .map_err(|e| e.to_string())
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())?;
        if current.trim() != saved_trimmed {
            new_text = current;
            break;
        }
    }

    if new_text.trim().is_empty() {
        info!("[Clipboard] 剪贴板内容未变化，可能没有选中文本");
        return Ok(String::new());
    }

    // Restore old clipboard content (best effort).
    if let Some(ref old_text) = saved_clipboard {
        let ps_script = format!("Set-Clipboard -Value '{}'", old_text.replace('\'', "''"));
        let _ = powershell_command(&ps_script).output();
        info!("[Clipboard] 原剪贴板内容已恢复");
    }

    info!(
        "[Clipboard] 选中文字已获取 (剪贴板回退), 文本长度={}",
        new_text.len()
    );
    Ok(new_text)
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
            powershell_command("Get-Clipboard")
                .output()
                .map_err(|e| e.to_string())
                .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
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
