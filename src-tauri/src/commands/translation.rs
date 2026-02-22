use crate::config::AppState;
use crate::translation::OpenAiCompatProvider;
use log::{info, error};
use tauri::State;

/// Translate text using the configured translation service.
#[tauri::command]
pub async fn translate_text(
    state: State<'_, AppState>,
    text: String,
    source_lang: String,
    target_lang: String,
) -> Result<String, String> {
    info!("[Translation] translate_text 开始, {} → {}, 文本长度={}", source_lang, target_lang, text.len());
    let (base_url, api_key, model, extra) = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        (
            settings.base_url.clone(),
            settings.api_key.clone(),
            settings.translation.model.clone(),
            settings.translation.extra.clone(),
        )
    };
    let client = state.http_client.clone();
    info!("[Translation] 使用 model={}, base_url={}", model, base_url);

    let provider = OpenAiCompatProvider::new(client);
    let result = provider
        .translate(&text, &source_lang, &target_lang, &base_url, &api_key, &model, &extra)
        .await
        .map_err(|e| e.to_string());
    match &result {
        Ok(translated) => info!("[Translation] 翻译完成, 结果长度={}", translated.len()),
        Err(e) => error!("[Translation] 翻译失败: {}", e),
    }
    result
}
