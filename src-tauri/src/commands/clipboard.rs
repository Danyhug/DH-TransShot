use log::{info, error, warn};

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
                    info!("[Clipboard] Accessibility API 获取成功, 文本长度={}", text.len());
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

    let text = String::from_utf8(output.stdout)
        .map_err(|e| format!("UTF-8 decode failed: {}", e))?
        .trim_end()
        .to_string();

    Ok(text)
}

/// Fallback: save clipboard → simulate Cmd/Ctrl+C → read new clipboard → restore old clipboard.
fn get_selected_text_clipboard_fallback() -> Result<String, String> {
    info!("[Clipboard] 剪贴板回退: 保存当前剪贴板 → 模拟复制 → 读取 → 恢复");

    // Step 1: Save current clipboard content
    #[cfg(target_os = "macos")]
    let saved_clipboard: Option<String> = std::process::Command::new("pbpaste")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok());

    #[cfg(target_os = "windows")]
    let saved_clipboard: Option<String> = std::process::Command::new("powershell")
        .args(["-command", "Get-Clipboard"])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok());

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let saved_clipboard: Option<String> = None;

    // Step 2: Simulate Cmd+C / Ctrl+C
    #[cfg(target_os = "macos")]
    {
        let copy_result = std::process::Command::new("osascript")
            .args(["-e", "tell application \"System Events\" to keystroke \"c\" using command down"])
            .output()
            .map_err(|e| format!("osascript failed: {}", e))?;

        if !copy_result.status.success() {
            let stderr = String::from_utf8_lossy(&copy_result.stderr);
            warn!("[Clipboard] 模拟复制失败: {}", stderr);
        }
    }

    #[cfg(target_os = "windows")]
    {
        let copy_result = std::process::Command::new("powershell")
            .args(["-command", "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait(\"^c\")"])
            .output()
            .map_err(|e| format!("powershell failed: {}", e))?;

        if !copy_result.status.success() {
            let stderr = String::from_utf8_lossy(&copy_result.stderr);
            warn!("[Clipboard] 模拟复制失败: {}", stderr);
        }
    }

    // Step 3: Wait for clipboard to update
    std::thread::sleep(std::time::Duration::from_millis(150));

    // Step 4: Read the new clipboard content
    #[cfg(target_os = "macos")]
    let new_clipboard: Result<String, String> = std::process::Command::new("pbpaste")
        .output()
        .map_err(|e| e.to_string())
        .and_then(|out| String::from_utf8(out.stdout).map_err(|e| e.to_string()));

    #[cfg(target_os = "windows")]
    let new_clipboard: Result<String, String> = std::process::Command::new("powershell")
        .args(["-command", "Get-Clipboard"])
        .output()
        .map_err(|e| e.to_string())
        .and_then(|out| String::from_utf8(out.stdout).map_err(|e| e.to_string()));

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let new_clipboard: Result<String, String> = Err("Clipboard not supported on this platform".to_string());

    let new_text = new_clipboard?;

    // Step 5: If clipboard didn't change, no text was selected
    if new_text.trim() == saved_clipboard.as_deref().unwrap_or("").trim() {
        info!("[Clipboard] 剪贴板内容未变化，可能没有选中文本");
        return Ok(String::new());
    }

    // Step 6: Restore old clipboard content (best effort)
    #[cfg(target_os = "macos")]
    if let Some(ref old_text) = saved_clipboard {
        let _ = std::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    let _ = stdin.write_all(old_text.as_bytes());
                }
                child.wait()
            });
        info!("[Clipboard] 原剪贴板内容已恢复");
    }

    #[cfg(target_os = "windows")]
    if let Some(ref old_text) = saved_clipboard {
        let ps_script = format!("Set-Clipboard -Value '{}'", old_text.replace('\'', "''"));
        let _ = std::process::Command::new("powershell")
            .args(["-command", &ps_script])
            .output();
        info!("[Clipboard] 原剪贴板内容已恢复");
    }

    info!("[Clipboard] 选中文字已获取 (剪贴板回退), 文本长度={}", new_text.len());
    Ok(new_text)
}

/// Read text from the system clipboard.
#[tauri::command]
pub async fn read_clipboard() -> Result<String, String> {
    info!("[Clipboard] read_clipboard 请求");
    let result = tokio::task::spawn_blocking(|| {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("pbpaste")
                .output()
                .map_err(|e| e.to_string())
                .and_then(|out| {
                    String::from_utf8(out.stdout).map_err(|e| e.to_string())
                })
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("powershell")
                .args(["-command", "Get-Clipboard"])
                .output()
                .map_err(|e| e.to_string())
                .and_then(|out| {
                    String::from_utf8(out.stdout).map_err(|e| e.to_string())
                })
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

/// Copy an image (base64 PNG) to the system clipboard.
#[tauri::command]
pub async fn copy_image_to_clipboard(image_base64: String) -> Result<(), String> {
    info!("[Clipboard] copy_image_to_clipboard, base64 size={}", image_base64.len());
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

        info!("[Clipboard] 临时文件已写入: {:?}, size={}", tmp_path, png_bytes.len());

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
            let output = std::process::Command::new("powershell")
                .args(["-command", &ps_script])
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
