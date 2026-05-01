use base64::Engine;
use crate::config::AppState;
use log::{info, error};
use tauri::State;

/// OCR: recognize text from a base64 image using configured OCR service.
#[tauri::command]
pub async fn recognize_text(
    state: State<'_, AppState>,
    image_base64: String,
    language: String,
) -> Result<String, String> {
    info!("[OCR] recognize_text 开始, language={}, image base64 size={}", language, image_base64.len());
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
    info!("[OCR] 使用 API: {}, model={}", base_url, model);

    let image_bytes = base64::engine::general_purpose::STANDARD.decode(&image_base64)
        .map_err(|e| e.to_string())?;
    let result = crate::ocr::recognize(&client, &image_bytes, &language, &base_url, &api_key, &model, &extra)
        .await
        .map_err(|e| e.to_string());
    match &result {
        Ok(text) => info!("[OCR] 识别完成, 结果长度={}, 内容: {}", text.len(), &text[..text.len().min(80)]),
        Err(e) => error!("[OCR] 识别失败: {}", e),
    }
    result
}

/// Combined capture + OCR: crop region from frozen screenshot and recognize text
/// in a single step. Avoids the PNG encode/decode round-trip of separate
/// captureRegion + recognizeText calls.
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
