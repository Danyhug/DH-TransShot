use crate::config::merge_extra;
use base64::Engine;
use log::{error, info, warn};
use reqwest::Client;

/// Build the full Audio Speech endpoint URL from a base_url.
fn audio_speech_url(base_url: &str) -> String {
    format!("{}/v1/audio/speech", base_url.trim_end_matches('/'))
}

/// Call the OpenAI-compatible TTS endpoint and return the audio as base64.
///
/// Sends `POST {base_url}/v1/audio/speech` with `{ model, input, response_format }`.
/// The `extra` JSON string is merged into the request body, allowing users to
/// customize `voice`, `speed`, and other provider-specific parameters.
///
/// Returns the raw audio bytes encoded as a base64 string.
pub async fn synthesize(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    extra: &str,
    text: &str,
) -> anyhow::Result<String> {
    if base_url.trim().is_empty() {
        anyhow::bail!("TTS 未配置 API 地址，请在设置中填写 base_url");
    }

    let url = audio_speech_url(base_url);
    info!("[TTS] 发送请求到 {}, model={}, 文本长度={}", url, model, text.len());

    // Default voice: "{model}:alex", can be overridden via extra
    let default_voice = format!("{}:alex", model);

    let mut request_body = serde_json::json!({
        "model": model,
        "input": text,
        "voice": default_voice,
        "response_format": "mp3"
    });

    // extra can override voice, speed, gain, etc.
    merge_extra(&mut request_body, extra, "TTS");

    let mut req = client.post(&url).json(&request_body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    } else {
        warn!("[TTS] API Key 为空");
    }

    let response = req.send().await?;
    let status = response.status();

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        error!("[TTS] API 错误 ({}): {}", status, body);
        anyhow::bail!("TTS API error ({}): {}", status, body);
    }

    let bytes = response.bytes().await?;
    info!("[TTS] 收到音频数据, 大小={}bytes", bytes.len());

    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(b64)
}
