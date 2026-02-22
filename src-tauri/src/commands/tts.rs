use crate::config::AppState;
use log::{error, info};
use tauri::State;

/// Synthesize speech from text using the configured TTS service.
/// Returns base64-encoded audio data (mp3).
#[tauri::command]
pub async fn synthesize_speech(
    state: State<'_, AppState>,
    text: String,
) -> Result<String, String> {
    info!("[TTS] synthesize_speech 开始, 文本长度={}", text.len());

    let (base_url, api_key, model, extra) = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        (
            settings.base_url.clone(),
            settings.api_key.clone(),
            settings.tts.model.clone(),
            settings.tts.extra.clone(),
        )
    };
    let client = state.http_client.clone();
    info!("[TTS] 使用 model={}, base_url={}", model, base_url);

    let result = crate::tts::synthesize(&client, &base_url, &api_key, &model, &extra, &text)
        .await
        .map_err(|e| e.to_string());

    match &result {
        Ok(b64) => info!("[TTS] 语音合成完成, base64长度={}", b64.len()),
        Err(e) => error!("[TTS] 语音合成失败: {}", e),
    }
    result
}
