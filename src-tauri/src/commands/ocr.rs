use crate::config::AppState;
use log::{info, error};
use tauri::State;

/// Combined capture + OCR: crop region from frozen screenshot and recognize text in a single step.
#[tauri::command]
pub async fn capture_and_ocr(
    state: State<'_, AppState>,
    monitor_index: usize,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    language: String,
) -> Result<String, String> {
    info!("[OCR] capture_and_ocr 开始, monitor={}, region=({},{},{}x{}), language={}", monitor_index, x, y, width, height, language);

    let base64 = {
        let guard = state.frozen_screenshots.lock().map_err(|e| e.to_string())?;
        guard
            .get(monitor_index)
            .cloned()
            .ok_or_else(|| format!("No frozen screenshot for monitor {}", monitor_index))?
    };

    let (base_url, api_key, model, extra) = {
        let guard = state.settings.lock().map_err(|e| e.to_string())?;
        (
            guard.base_url.clone(),
            guard.api_key.clone(),
            guard.ocr.model.clone(),
            guard.ocr.extra.clone(),
        )
    };
    let client = state.http_client.clone();

    // Crop region and get raw JPEG bytes in one step (no intermediate base64/PNG)
    let image_bytes = tokio::task::spawn_blocking(move || {
        crate::screenshot::capture_region_bytes(&base64, x, y, width, height)
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())?;

    info!("[OCR] 裁切完成, JPEG bytes={}, 开始 OCR 识别", image_bytes.len());

    let result = crate::ocr::recognize(&client, &image_bytes, &language, &base_url, &api_key, &model, &extra)
        .await
        .map_err(|e| e.to_string());

    match &result {
        Ok(text) => info!("[OCR] capture_and_ocr 完成, 结果长度={}", text.len()),
        Err(e) => error!("[OCR] capture_and_ocr 失败: {}", e),
    }
    result
}
