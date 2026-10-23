use crate::config::AppState;
use crate::config::MonitorInfo;
use crate::screenshot::OVERLAY_LABEL_PREFIX;
use crate::window_lifecycle::{
    close_many_deferred, close_overlays_deferred, TEARDOWN_SETTLE_DELAY,
};
use log::{error, info};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

/// 「按下快捷键 → 覆盖层出现」这条链路的计时锚点。
///
/// 日志时间戳只有秒级精度，跨进程（热键线程 / 主窗口 JS / 覆盖层 JS）根本对不齐，
/// 所以每一段都用本进程的 `Instant` 计时。锚点之间空出来的部分就是没打点的开销：
/// 热键派发 + 等 Alt 松开 + 主窗口事件 → invoke、以及建窗 → 覆盖层页面加载完成。
static HOTKEY_TRIGGERED_AT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
static OVERLAY_CREATED_AT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

fn hotkey_triggered_slot() -> &'static Mutex<Option<Instant>> {
    HOTKEY_TRIGGERED_AT.get_or_init(|| Mutex::new(None))
}

fn overlay_created_slot() -> &'static Mutex<Option<Instant>> {
    OVERLAY_CREATED_AT.get_or_init(|| Mutex::new(None))
}

/// 热键回调里调用，记录「用户按下快捷键」的时刻。
pub fn mark_hotkey_triggered() {
    if let Ok(mut slot) = hotkey_triggered_slot().lock() {
        *slot = Some(Instant::now());
    }
}

/// 距按下快捷键过去了多少 ms；没有锚点（比如从托盘菜单或标题栏按钮触发）时为 None。
fn since_hotkey_ms() -> Option<u128> {
    let triggered_at = hotkey_triggered_slot().lock().ok()?.as_ref().copied()?;
    Some(triggered_at.elapsed().as_millis())
}

fn mark_overlay_created() {
    if let Ok(mut slot) = overlay_created_slot().lock() {
        *slot = Some(Instant::now());
    }
}

/// 距覆盖层窗口创建过去了多少 ms。这段时间是 webview 创建 + 页面加载 + React 挂载。
fn since_overlay_created_ms() -> Option<u128> {
    let created_at = overlay_created_slot().lock().ok()?.as_ref().copied()?;
    Some(created_at.elapsed().as_millis())
}

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
    let request_started_at = std::time::Instant::now();
    info!(
        "[Screenshot] start_region_select, mode={}, 距按下快捷键={}ms",
        mode,
        since_hotkey_ms().map_or("n/a(非热键触发)".to_string(), |ms| ms.to_string())
    );

    // Guard: close any existing overlay windows
    let has_existing = app
        .webview_windows()
        .keys()
        .any(|k| k.starts_with(OVERLAY_LABEL_PREFIX));
    if has_existing {
        info!("[Screenshot] 覆盖层窗口已存在，先关闭旧窗口");
        // 自带让帧等待，销毁完成后才继续建新窗口
        let close_started_at = std::time::Instant::now();
        close_overlays_deferred(&app, None).await;
        info!(
            "[Screenshot] 旧覆盖层已销毁, 耗时={}ms",
            close_started_at.elapsed().as_millis()
        );
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
        let close_started_at = std::time::Instant::now();
        close_many_deferred(stale_windows).await;
        info!(
            "[Screenshot] settings / debug-log 已销毁, 耗时={}ms",
            close_started_at.elapsed().as_millis()
        );
    }

    // 1. Collect all monitors info
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

    // 2. Collect window rects and freeze the screens in parallel: both need a
    // WindowServer round trip and neither depends on the other. Neither may run
    // after the overlay windows exist.
    let window_rects_started_at = std::time::Instant::now();
    let window_rects_task = tokio::task::spawn_blocking(crate::screenshot::list_window_rects);

    info!("[Screenshot] 开始逐显示器截图...");
    let capture_started_at = std::time::Instant::now();
    let capture_result =
        tokio::task::spawn_blocking(move || crate::screenshot::capture_monitors(&logical_rects));

    let window_rects = window_rects_task.await.map_err(|e| e.to_string())?;
    info!(
        "[Screenshot] 窗口列表采集完成, count={}, 耗时={}ms",
        window_rects.len(),
        window_rects_started_at.elapsed().as_millis()
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

    let capture_result = capture_result.await.map_err(|e| e.to_string())?;
    info!(
        "[Screenshot] 逐显示器截图 + PNG 编码完成, 耗时={}ms",
        capture_started_at.elapsed().as_millis()
    );

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

    // 3. Create overlay windows for each monitor
    let window_creation_started_at = std::time::Instant::now();
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
        "[Screenshot] 所有覆盖层窗口已创建, count={}, 建窗耗时={}ms, 命令总耗时={}ms",
        monitors.len(),
        window_creation_started_at.elapsed().as_millis(),
        request_started_at.elapsed().as_millis()
    );
    mark_overlay_created();

    Ok(())
}

/// Get the frozen screenshot metadata for a specific monitor's overlay window.
///
/// 图片本身不走这里：它由 `get_frozen_screenshot_image` 以二进制返回。以前把
/// base64 塞进这个 JSON，一张 2560x1664 的 Retina 截图就是 14MB 字符串，serde 要
/// 序列化一遍、IPC 要传一遍、前端再 `JSON.parse` 一遍，全是纯浪费。
#[tauri::command]
pub async fn get_frozen_screenshot(
    state: State<'_, AppState>,
    monitor_index: usize,
) -> Result<serde_json::Value, String> {
    info!(
        "[Screenshot] get_frozen_screenshot 请求, monitor_index={}, 距建窗={}ms",
        monitor_index,
        since_overlay_created_ms().map_or("n/a".to_string(), |ms| ms.to_string())
    );
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

    info!(
        "[Screenshot] get_frozen_screenshot 返回, monitor_index={}, mode={}, monitors={}",
        monitor_index,
        mode,
        monitors.len()
    );

    Ok(serde_json::json!({
        "mode": mode,
        "window_rects": window_rects,
        "monitors": monitors,
    }))
}

/// Get the frozen screenshot PNG for a monitor as raw bytes.
///
/// 返回 `tauri::ipc::Response` 的 Raw body，前端直接拿到 `ArrayBuffer`：省掉 base64
/// 的 33% 膨胀、JSON 转义/解析，以及前端 `atob` + 逐字节 `charCodeAt` 循环。
#[tauri::command]
pub async fn get_frozen_screenshot_image(
    state: State<'_, AppState>,
    monitor_index: usize,
) -> Result<tauri::ipc::Response, String> {
    let base64 = {
        let guard = state.frozen_screenshots.lock().map_err(|e| e.to_string())?;
        guard
            .get(monitor_index)
            .cloned()
            .ok_or_else(|| format!("No frozen screenshot for monitor {}", monitor_index))?
    };

    let started_at = std::time::Instant::now();
    let bytes = tokio::task::spawn_blocking(move || {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.decode(base64)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    info!(
        "[Screenshot] get_frozen_screenshot_image, monitor_index={}, bytes={}, 解码耗时={}ms",
        monitor_index,
        bytes.len(),
        started_at.elapsed().as_millis()
    );

    Ok(tauri::ipc::Response::new(bytes))
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
