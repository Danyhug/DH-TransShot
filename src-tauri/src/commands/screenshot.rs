use crate::config::AppState;
use log::{info, error};
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

/// Start region selection: move main window off-screen, capture full screen,
/// store it in AppState, then create the screenshot overlay window.
#[tauri::command]
pub async fn start_region_select(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mode: String,
) -> Result<(), String> {
    info!("[Screenshot] start_region_select, mode={}", mode);

    // Guard: if the overlay window already exists, skip duplicate invocation
    if app.get_webview_window("screenshot-overlay").is_some() {
        info!("[Screenshot] 覆盖层窗口已存在，忽略重复调用");
        return Ok(());
    }

    // 0. Read hide_on_capture setting
    let hide_on_capture = {
        let guard = state.settings.lock().map_err(|e| e.to_string())?;
        guard.hide_on_capture
    };
    info!("[Screenshot] hide_on_capture={}", hide_on_capture);

    // 0.5 Close settings and debug-log windows (they would obscure the overlay)
    if let Some(w) = app.get_webview_window("settings") {
        info!("[Screenshot] 关闭 settings 窗口");
        let _ = w.close();
    }
    if let Some(w) = app.get_webview_window("debug-log") {
        info!("[Screenshot] 关闭 debug-log 窗口");
        let _ = w.close();
    }

    // 1. Move main window off-screen instead of hide/minimize (only if hide_on_capture).
    let saved_pos = if hide_on_capture {
        if let Some(main_win) = app.get_webview_window("main") {
            let pos = main_win.outer_position().unwrap_or(tauri::PhysicalPosition::new(100, 100));
            info!("[Screenshot] 主窗口移至屏幕外, 原位置=({}, {})", pos.x, pos.y);
            let _ = main_win.set_position(tauri::Position::Physical(
                tauri::PhysicalPosition::new(-20000, -20000),
            ));
            Some(pos)
        } else {
            error!("[Screenshot] 找不到主窗口");
            None
        }
    } else {
        info!("[Screenshot] hide_on_capture=false, 不移动主窗口");
        None
    };

    // 2. Brief delay for the window move to take effect
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 3. Capture the screen
    info!("[Screenshot] 开始全屏截图...");
    let capture_result = tokio::task::spawn_blocking(|| crate::screenshot::capture_full())
        .await
        .map_err(|e| e.to_string())?;

    // If capture fails, restore the window position before returning error
    let full_base64 = match capture_result {
        Ok(data) => {
            info!("[Screenshot] 全屏截图完成, base64 size={}", data.len());
            data
        }
        Err(e) => {
            error!("[Screenshot] 全屏截图失败: {}", e);
            if let (Some(main_win), Some(pos)) =
                (app.get_webview_window("main"), saved_pos)
            {
                let _ = main_win.set_position(tauri::Position::Physical(pos));
            }
            return Err(e.to_string());
        }
    };

    {
        let mut guard = state.frozen_screenshot.lock().map_err(|e| e.to_string())?;
        *guard = Some(full_base64);
    }
    {
        let mut guard = state.frozen_mode.lock().map_err(|e| e.to_string())?;
        *guard = mode.clone();
    }

    let monitor = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("No primary monitor")?;

    let size = monitor.size();
    let scale = monitor.scale_factor();
    let logical_width = size.width as f64 / scale;
    let logical_height = size.height as f64 / scale;

    info!("[Screenshot] 创建覆盖层窗口, 显示器={}x{}, scale={}, logical={}x{}", size.width, size.height, scale, logical_width, logical_height);

    // 4. Create the screenshot overlay window
    let overlay = WebviewWindowBuilder::new(
        &app,
        "screenshot-overlay",
        WebviewUrl::App("screenshot.html".into()),
    )
    .title("Screenshot")
    .inner_size(logical_width, logical_height)
    .position(0.0, 0.0)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .build()
    .map_err(|e: tauri::Error| e.to_string())?;

    info!("[Screenshot] 覆盖层窗口已创建");

    // 5. Restore main window position when overlay closes
    //    - screenshot mode: restore position only (don't show/focus)
    //    - ocr_translate mode: restore position + show + focus
    let frozen_mode = mode.clone();
    let app_clone = app.clone();
    overlay.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            info!("[Screenshot] 覆盖层窗口关闭, mode={}", frozen_mode);
            if let Some(main_win) = app_clone.get_webview_window("main") {
                if let Some(pos) = saved_pos {
                    let _ = main_win.set_position(tauri::Position::Physical(pos));
                }
                if frozen_mode == "ocr_translate" {
                    info!("[Screenshot] ocr_translate 模式，显示主窗口");
                    let _ = main_win.show();
                    let _ = main_win.set_focus();
                } else {
                    info!("[Screenshot] screenshot 模式，不显示主窗口");
                }
            }
        }
    });

    Ok(())
}

/// Get the frozen screenshot data for the overlay window.
#[tauri::command]
pub async fn get_frozen_screenshot(
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    info!("[Screenshot] get_frozen_screenshot 请求");
    let image = {
        let guard = state.frozen_screenshot.lock().map_err(|e| e.to_string())?;
        guard
            .clone()
            .ok_or_else(|| "No frozen screenshot available".to_string())?
    };
    let mode = {
        let guard = state.frozen_mode.lock().map_err(|e| e.to_string())?;
        guard.clone()
    };

    info!("[Screenshot] get_frozen_screenshot 返回, mode={}, image size={}", mode, image.len());

    Ok(serde_json::json!({
        "image": image,
        "mode": mode,
    }))
}

/// Capture a region from the frozen full-screen screenshot.
#[tauri::command]
pub async fn capture_region(
    state: State<'_, AppState>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<String, String> {
    info!("[Screenshot] capture_region, region=({},{},{}x{})", x, y, width, height);
    let full_base64 = {
        let guard = state.frozen_screenshot.lock().map_err(|e| e.to_string())?;
        guard
            .clone()
            .ok_or_else(|| "No frozen screenshot available".to_string())?
    };

    let result = crate::screenshot::capture_region_from_full(&full_base64, x, y, width, height)
        .map_err(|e| e.to_string());
    match &result {
        Ok(data) => info!("[Screenshot] capture_region 完成, 裁切后 base64 size={}", data.len()),
        Err(e) => error!("[Screenshot] capture_region 失败: {}", e),
    }
    result
}
