use crate::config::AppState;
use log::{info, error};
use tauri::{Listener, Manager, State, WebviewUrl, WebviewWindowBuilder};

/// Monitor information passed to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
struct MonitorInfo {
    name: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    scale_factor: f64,
}

/// Close all existing screenshot overlay windows (labels matching "screenshot-overlay-*").
fn close_all_overlays(app: &tauri::AppHandle) {
    for win in app.webview_windows().values() {
        if win.label().starts_with("screenshot-overlay") {
            info!("[Screenshot] 关闭覆盖层窗口: {}", win.label());
            let _ = win.close();
        }
    }
}

/// Start region selection: capture per-monitor screenshots, store them in AppState,
/// then create screenshot overlay windows for ALL monitors.
#[tauri::command]
pub async fn start_region_select(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mode: String,
) -> Result<(), String> {
    info!("[Screenshot] start_region_select, mode={}", mode);

    // Guard: close any existing overlay windows
    let has_existing = app.webview_windows().keys().any(|k| k.starts_with("screenshot-overlay"));
    if has_existing {
        info!("[Screenshot] 覆盖层窗口已存在，先关闭旧窗口");
        close_all_overlays(&app);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    // 0. Close settings and debug-log windows (they would obscure the overlay)
    if let Some(w) = app.get_webview_window("settings") {
        info!("[Screenshot] 关闭 settings 窗口");
        let _ = w.close();
    }
    if let Some(w) = app.get_webview_window("debug-log") {
        info!("[Screenshot] 关闭 debug-log 窗口");
        let _ = w.close();
    }

    // 1. Brief delay before capture
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 2. Collect window rects BEFORE creating the overlay window
    info!("[Screenshot] 采集窗口列表...");
    let window_rects = tokio::task::spawn_blocking(|| crate::screenshot::list_window_rects())
        .await
        .map_err(|e| e.to_string())?;
    info!("[Screenshot] 窗口列表采集完成, count={}", window_rects.len());

    {
        let rects_json = serde_json::to_value(&window_rects).unwrap_or(serde_json::Value::Array(vec![]));
        let mut guard = state.frozen_window_rects.lock().map_err(|e| e.to_string())?;
        *guard = rects_json;
    }

    // 3. Collect all monitors info
    let monitors = app.available_monitors().map_err(|e| e.to_string())?;
    if monitors.is_empty() {
        return Err("No monitors found".to_string());
    }

    let mut monitor_infos: Vec<MonitorInfo> = Vec::new();
    let mut logical_rects: Vec<(f64, f64, f64, f64)> = Vec::new();
    for mon in &monitors {
        let pos = mon.position();
        let size = mon.size();
        let scale = mon.scale_factor();
        let name = mon.name().cloned().unwrap_or_default();
        monitor_infos.push(MonitorInfo {
            name,
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
            scale_factor: scale,
        });
        // Logical coordinates for per-monitor capture
        logical_rects.push((
            pos.x as f64 / scale,
            pos.y as f64 / scale,
            size.width as f64 / scale,
            size.height as f64 / scale,
        ));
    }

    info!("[Screenshot] 检测到 {} 个显示器", monitor_infos.len());
    for (i, m) in monitor_infos.iter().enumerate() {
        info!("[Screenshot] 显示器[{}]: name={}, pos=({},{}), size={}x{}, scale={}", i, m.name, m.x, m.y, m.width, m.height, m.scale_factor);
    }

    // 4. Capture each monitor individually (native resolution per monitor)
    info!("[Screenshot] 开始逐显示器截图...");
    let rects_clone = logical_rects.clone();
    let capture_result = tokio::task::spawn_blocking(move || {
        crate::screenshot::capture_monitors(&rects_clone)
    })
    .await
    .map_err(|e| e.to_string())?;

    let screenshots = match capture_result {
        Ok(data) => {
            info!("[Screenshot] 逐显示器截图完成, count={}", data.len());
            for (i, s) in data.iter().enumerate() {
                info!("[Screenshot] 显示器[{}] base64 size={}", i, s.len());
            }
            data
        }
        Err(e) => {
            error!("[Screenshot] 截图失败: {}", e);
            return Err(e.to_string());
        }
    };

    // Store per-monitor screenshots
    {
        let mut guard = state.frozen_screenshots.lock().map_err(|e| e.to_string())?;
        *guard = screenshots;
    }
    {
        let mut guard = state.frozen_mode.lock().map_err(|e| e.to_string())?;
        *guard = mode.clone();
    }

    // Store monitor info in AppState
    {
        let monitors_json: Vec<serde_json::Value> = monitor_infos
            .iter()
            .map(|m| serde_json::to_value(m).unwrap_or(serde_json::Value::Null))
            .collect();
        let mut guard = state.frozen_monitors.lock().map_err(|e| e.to_string())?;
        *guard = monitors_json;
    }

    // 5. Create overlay windows for each monitor
    for (i, mon) in monitors.iter().enumerate() {
        let label = format!("screenshot-overlay-{}", i);
        let pos = mon.position();
        let size = mon.size();
        let scale = mon.scale_factor();
        let logical_w = size.width as f64 / scale;
        let logical_h = size.height as f64 / scale;

        info!(
            "[Screenshot] 创建覆盖层窗口[{}]: pos=({},{}), logical={}x{}, scale={}",
            i, pos.x, pos.y, logical_w, logical_h, scale
        );

        let build_overlay = || {
            WebviewWindowBuilder::new(
                &app,
                &label,
                WebviewUrl::App("screenshot.html".into()),
            )
            .title("Screenshot")
            .inner_size(logical_w, logical_h)
            .position(pos.x as f64 / scale, pos.y as f64 / scale)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .build()
        };

        let overlay = match build_overlay() {
            Ok(w) => w,
            Err(_) => {
                info!("[Screenshot] 覆盖层[{}]创建失败，尝试关闭残留窗口后重试", i);
                if let Some(existing) = app.get_webview_window(&label) {
                    let _ = existing.close();
                }
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                build_overlay().map_err(|e: tauri::Error| e.to_string())?
            }
        };

        // Show main window when the FIRST overlay closes (ocr_translate mode)
        if i == 0 {
            let frozen_mode = mode.clone();
            let app_clone = app.clone();
            overlay.on_window_event(move |event| {
                if let tauri::WindowEvent::Destroyed = event {
                    info!("[Screenshot] 主覆盖层窗口关闭, mode={}", frozen_mode);
                    if frozen_mode == "ocr_translate" {
                        if let Some(main_win) = app_clone.get_webview_window("main") {
                            info!("[Screenshot] ocr_translate 模式，显示主窗口");
                            let _ = main_win.show();
                            let _ = main_win.set_focus();
                        }
                    }
                }
            });
        }
    }

    info!("[Screenshot] 所有覆盖层窗口已创建, count={}", monitors.len());

    // 6. Listen for close-all-overlays event
    let app_clone = app.clone();
    app.listen("close-all-overlays", move |_| {
        info!("[Screenshot] 收到 close-all-overlays 事件");
        close_all_overlays(&app_clone);
    });

    Ok(())
}

/// Get the frozen screenshot data for a specific monitor's overlay window.
#[tauri::command]
pub async fn get_frozen_screenshot(
    state: State<'_, AppState>,
    monitor_index: usize,
) -> Result<serde_json::Value, String> {
    info!("[Screenshot] get_frozen_screenshot 请求, monitor_index={}", monitor_index);
    let image = {
        let guard = state.frozen_screenshots.lock().map_err(|e| e.to_string())?;
        guard
            .get(monitor_index)
            .cloned()
            .ok_or_else(|| format!("No frozen screenshot for monitor {}", monitor_index))?
    };
    let mode = {
        let guard = state.frozen_mode.lock().map_err(|e| e.to_string())?;
        guard.clone()
    };
    let window_rects = {
        let guard = state.frozen_window_rects.lock().map_err(|e| e.to_string())?;
        guard.clone()
    };
    let monitors = {
        let guard = state.frozen_monitors.lock().map_err(|e| e.to_string())?;
        guard.clone()
    };

    info!("[Screenshot] get_frozen_screenshot 返回, monitor_index={}, mode={}, image size={}, monitors={}", monitor_index, mode, image.len(), monitors.len());

    Ok(serde_json::json!({
        "image": image,
        "mode": mode,
        "window_rects": window_rects,
        "monitors": monitors,
    }))
}

/// Capture a region from a specific monitor's frozen screenshot.
#[tauri::command]
pub async fn capture_region(
    state: State<'_, AppState>,
    monitor_index: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<String, String> {
    info!("[Screenshot] capture_region, monitor_index={}, region=({},{},{}x{})", monitor_index, x, y, width, height);
    let base64 = {
        let guard = state.frozen_screenshots.lock().map_err(|e| e.to_string())?;
        guard
            .get(monitor_index)
            .cloned()
            .ok_or_else(|| format!("No frozen screenshot for monitor {}", monitor_index))?
    };

    let result = crate::screenshot::capture_region_from_full(&base64, x, y, width, height)
        .map_err(|e| e.to_string());
    match &result {
        Ok(data) => info!("[Screenshot] capture_region 完成, 裁切后 base64 size={}", data.len()),
        Err(e) => error!("[Screenshot] capture_region 失败: {}", e),
    }
    result
}
