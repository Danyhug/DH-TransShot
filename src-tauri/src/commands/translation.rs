use crate::config::AppState;
use crate::translation::OpenAiCompatProvider;
use log::{error, info};
use tauri::State;

/// Translate text using the configured translation service.
#[tauri::command]
pub async fn translate_text(
    state: State<'_, AppState>,
    text: String,
    source_lang: String,
    target_lang: String,
) -> Result<String, String> {
    info!(
        "[Translation] translate_text 开始, {} → {}, 文本长度={}",
        source_lang,
        target_lang,
        text.len()
    );
    let ((base_url, api_key, model, extra), prefs) = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        (
            settings
                .translation
                .resolved(&settings.base_url, &settings.api_key),
            settings.translation_prompt.clone(),
        )
    };
    let client = state.http_client.clone();
    info!("[Translation] 使用 model={}, base_url={}", model, base_url);
    info!(
        "[Translation] 提示词偏好: 解释缩写={}, 行业={:?}, 自定义提示词长度={}",
        prefs.expand_abbreviations,
        prefs.domains,
        prefs.custom_prompt.trim().len()
    );

    let provider = OpenAiCompatProvider::new(client);
    let result = provider
        .translate(
            &text,
            &source_lang,
            &target_lang,
            &base_url,
            &api_key,
            &model,
            &extra,
            &prefs,
        )
        .await
        .map_err(|e| e.to_string());
    match &result {
        Ok(translated) => info!("[Translation] 翻译完成, 结果长度={}", translated.len()),
        Err(e) => error!("[Translation] 翻译失败: {}", e),
    }
    result
}
