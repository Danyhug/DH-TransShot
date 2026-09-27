use crate::config::AppState;
use crate::config::MonitorInfo;
use crate::screenshot::OVERLAY_LABEL_PREFIX;
use crate::window_lifecycle::{
    close_many_deferred, close_overlays_deferred, TEARDOWN_SETTLE_DELAY,
};
use log::{error, info};
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

/// Close all existing screenshot overlay windows (labels matching "screenshot-overlay-*").
///
/// 真正的销毁是异步且带让帧的（见 `window_lifecycle`），这里只负责发起，所以从事件
/// 回调里调用不会阻塞，也不会在 display link 刷新过程中拆掉 webview。
pub fn close_all_overlays(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        close_overlays_deferred(&app, None).await;
    });
}

/// Close all screenshot overlay windows except the one with label `keep_label`.
pub fn close_other_overlays(app: &tauri::AppHandle, keep_label: &str) {
    let app = app.clone();
    let keep_label = keep_label.to_string();
    tauri::async_runtime::spawn(async move {
        close_overlays_deferred(&app, Some(&keep_label)).await;
    });
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
    let has_existing = app
        .webview_windows()
        .keys()
        .any(|k| k.starts_with(OVERLAY_LABEL_PREFIX));
    if has_existing {
        info!("[Screenshot] 覆盖层窗口已存在，先关闭旧窗口");
        // 自带让帧等待，销毁完成后才继续建新窗口
        close_overlays_deferred(&app, None).await;
    }

    // 0. Close settings and debug-log windows (they would obscure the overlay)
    let stale_windows: Vec<tauri::WebviewWindow> = ["settings", "debug-log"]
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .collect();
    if !stale_windows.is_empty() {
        info!(
            "[Screenshot] 关闭 settings / debug-log 窗口, count={}",
            stale_windows.len()
        );
        close_many_deferred(stale_windows).await;
    }

    // 1. Brief delay before capture
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 2. Collect window rects BEFORE creating the overlay window
    info!("[Screenshot] 采集窗口列表...");
    let window_rects = tokio::task::spawn_blocking(crate::screenshot::list_window_rects)
        .await
        .map_err(|e| e.to_string())?;
    info!(
        "[Screenshot] 窗口列表采集完成, count={}",
        window_rects.len()
    );

    {
        let rects_json =
            serde_json::to_value(&window_rects).unwrap_or(serde_json::Value::Array(vec![]));
        let mut guard = state
            .frozen_window_rects
            .lock()
            .map_err(|e| e.to_string())?;
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
        info!(
            "[Screenshot] 显示器[{}]: name={}, pos=({},{}), size={}x{}, scale={}",
            i, m.name, m.x, m.y, m.width, m.height, m.scale_factor
        );
    }

    // 4. Capture each monitor individually (native resolution per monitor)
    info!("[Screenshot] 开始逐显示器截图...");
    let capture_result =
        tokio::task::spawn_blocking(move || crate::screenshot::capture_monitors(&logical_rects))
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
        let mut guard = state.frozen_monitors.lock().map_err(|e| e.to_string())?;
        *guard = monitor_infos.clone();
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
            WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("screenshot.html".into()))
                .title("Screenshot")
                .inner_size(logical_w, logical_h)
                .position(pos.x as f64 / scale, pos.y as f64 / scale)
                .decorations(false)
                .resizable(false)
                .transparent(true)
                .shadow(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .visible(false)
                .accept_first_mouse(true)
                .build()
        };

        if build_overlay().is_err() {
            info!("[Screenshot] 覆盖层[{}]创建失败，尝试关闭残留窗口后重试", i);
            let stale: Vec<tauri::WebviewWindow> =
                app.get_webview_window(&label).into_iter().collect();
            close_many_deferred(stale).await;
            // 等窗口真正从窗口树上消失，复用同一个 label 才不会撞名
            tokio::time::sleep(TEARDOWN_SETTLE_DELAY).await;
            build_overlay().map_err(|e: tauri::Error| e.to_string())?;
        }
    }

    info!(
        "[Screenshot] 所有覆盖层窗口已创建, count={}",
        monitors.len()
    );

    Ok(())
}

/// Get the frozen screenshot data for a specific monitor's overlay window.
#[tauri::command]
pub async fn get_frozen_screenshot(
    state: State<'_, AppState>,
    monitor_index: usize,
) -> Result<serde_json::Value, String> {
    info!(
        "[Screenshot] get_frozen_screenshot 请求, monitor_index={}",
        monitor_index
    );
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
        let guard = state
            .frozen_window_rects
            .lock()
            .map_err(|e| e.to_string())?;
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
    info!(
        "[Screenshot] capture_region, monitor_index={}, region=({},{},{}x{})",
        monitor_index, x, y, width, height
    );
    let base64 = {
        let guard = state.frozen_screenshots.lock().map_err(|e| e.to_string())?;
        guard
            .get(monitor_index)
            .cloned()
            .ok_or_else(|| format!("No frozen screenshot for monitor {}", monitor_index))?
    };

    let result = tokio::task::spawn_blocking(move || {
        crate::screenshot::capture_region_from_full(&base64, x, y, width, height)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string());
    match &result {
        Ok(data) => info!(
            "[Screenshot] capture_region 完成, 裁切后 base64 size={}",
            data.len()
        ),
        Err(e) => error!("[Screenshot] capture_region 失败: {}", e),
    }
    result
}
