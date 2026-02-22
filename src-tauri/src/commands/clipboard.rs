use log::{info, error};

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
