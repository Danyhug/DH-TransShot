use crate::config::AppState;
use log::{error, info};
use tauri::State;

fn normalize_tts_text(text: &str) -> String {
    text.trim().replace("\r\n", "\n")
}

fn tts_cache_key(base_url: &str, model: &str, extra: &str, text: &str) -> String {
    format!("{base_url}\n{model}\n{extra}\n{text}")
}

/// Synthesize speech from text using the configured TTS service.
/// Returns base64-encoded audio data (mp3).
#[tauri::command]
pub async fn synthesize_speech(
    state: State<'_, AppState>,
    text: String,
) -> Result<String, String> {
    let normalized_text = normalize_tts_text(&text);
    info!(
        "[TTS] synthesize_speech 开始, 原始文本长度={}, 规范化后长度={}",
        text.len(),
        normalized_text.len()
    );

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
    let cache_key = tts_cache_key(&base_url, &model, &extra, &normalized_text);

    if let Some(cached) = state
        .tts_cache
        .lock()
        .map_err(|e| e.to_string())?
        .get(&cache_key)
    {
        info!("[TTS] 命中缓存, base64长度={}", cached.len());
        return Ok(cached);
    }
    info!("[TTS] 缓存未命中，发起语音合成");

    let result = crate::tts::synthesize(&client, &base_url, &api_key, &model, &extra, &normalized_text)
        .await
        .map_err(|e| e.to_string());

    match &result {
        Ok(b64) => {
            info!("[TTS] 语音合成完成, base64长度={}", b64.len());
            state
                .tts_cache
                .lock()
                .map_err(|e| e.to_string())?
                .insert(cache_key, b64.clone());
            info!("[TTS] 已写入缓存");
        }
        Err(e) => error!("[TTS] 语音合成失败: {}", e),
    }
    result
}
