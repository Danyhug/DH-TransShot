use crate::config::AppState;
use log::{info, error};
use tauri::State;

/// OCR: recognize text from a base64 PNG image using configured OCR service.
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

    let result = crate::ocr::recognize(&client, &image_base64, &language, &base_url, &api_key, &model, &extra)
        .await
        .map_err(|e| e.to_string());
    match &result {
        Ok(text) => info!("[OCR] 识别完成, 结果长度={}, 内容: {}", text.len(), &text[..text.len().min(80)]),
        Err(e) => error!("[OCR] 识别失败: {}", e),
    }
    result
}
