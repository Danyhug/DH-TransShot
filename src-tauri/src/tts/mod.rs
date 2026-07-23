use crate::config::merge_extra;
use base64::Engine;
use log::{error, info, warn};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

/// Build the full Audio Speech endpoint URL from a base_url.
/// 复用 `api_client::build_endpoint_url` 的自适应拼接规则（根/版本段/完整端点/`#` raw）。
fn audio_speech_url(base_url: &str) -> String {
    crate::api_client::build_endpoint_url(base_url, "audio/speech")
}

/// TTS 请求模式：
/// - `audio_speech`（默认）：OpenAI 标准 `/v1/audio/speech`，请求体 `{ model, input, voice, response_format }`，响应为二进制音频流。
/// - `chat_completions`：Chat Completions 格式 `/v1/chat/completions`，请求体含 `audio: { voice, format }`，
///   响应为 JSON，音频以 base64 嵌在 `choices[0].message.audio.data`。小米 MiMo 等 API 使用此格式。
fn resolve_tts_mode(extra: &str) -> &'static str {
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(extra.trim()) {
        if let Some(Value::String(mode)) = map.get("tts_mode") {
            match mode.as_str() {
                "chat_completions" => return "chat_completions",
                "audio_speech" => return "audio_speech",
                _ => {
                    warn!("[TTS] 未知 tts_mode: {}，回退到 audio_speech", mode);
                }
            }
        }
    }
    "audio_speech"
}

/// Chat Completions TTS 响应结构（小米 MiMo 等）。
#[derive(Deserialize)]
struct ChatTtsResponse {
    choices: Vec<ChatTtsChoice>,
}

#[derive(Deserialize)]
struct ChatTtsChoice {
    message: ChatTtsMessage,
}

#[derive(Deserialize)]
struct ChatTtsMessage {
    #[serde(default)]
    audio: Option<ChatTtsAudio>,
}

#[derive(Deserialize)]
struct ChatTtsAudio {
    #[serde(default)]
    data: Option<String>,
}

/// Call the TTS endpoint and return the audio as base64.
///
/// 支持两种 TTS API 格式，通过 `extra` 中的 `tts_mode` 字段切换：
///
/// - **`audio_speech`**（默认，OpenAI 标准）：
///   `POST {base_url}/v1/audio/speech`，请求体 `{ model, input, voice, response_format }`，
///   响应为二进制音频流，直接 base64 编码返回。
///
/// - **`chat_completions`**（小米 MiMo 等）：
///   `POST {base_url}/v1/chat/completions`，请求体含 `audio: { voice, format }`，
///   响应为 JSON，音频以 base64 嵌在 `choices[0].message.audio.data`。
///
/// `extra` JSON 字符串会合并到请求体中，可覆盖 `voice`、`speed` 等参数。
/// `tts_mode` 字段不会出现在最终请求体中（仅用于模式判断）。
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

    let mode = resolve_tts_mode(extra);
    info!("[TTS] 模式={}, model={}, 文本长度={}", mode, model, text.len());

    match mode {
        "chat_completions" => synthesize_via_chat_completions(client, base_url, api_key, model, extra, text).await,
        _ => synthesize_via_audio_speech(client, base_url, api_key, model, extra, text).await,
    }
}

/// OpenAI 标准 `/v1/audio/speech` 格式：响应为二进制音频流。
async fn synthesize_via_audio_speech(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    extra: &str,
    text: &str,
) -> anyhow::Result<String> {
    let url = audio_speech_url(base_url);
    info!("[TTS] 发送请求到 {} (audio_speech)", url);

    // Default voice: "{model}:alex", can be overridden via extra
    let default_voice = format!("{}:alex", model);

    let mut request_body = serde_json::json!({
        "model": model,
        "input": text,
        "voice": default_voice,
        "response_format": "mp3"
    });

    merge_extra(&mut request_body, extra, "TTS");
    // tts_mode 是内部控制字段，不发送给 API
    if let serde_json::Value::Object(ref mut map) = request_body {
        map.remove("tts_mode");
    }

    let b64 = send_and_extract_binary(client, &url, api_key, &request_body, "TTS").await?;
    info!("[TTS] 收到音频数据 (audio_speech), base64长度={}", b64.len());
    Ok(b64)
}

/// Chat Completions 格式（小米 MiMo 等）：响应为 JSON，音频以 base64 嵌在 message.audio.data。
async fn synthesize_via_chat_completions(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    extra: &str,
    text: &str,
) -> anyhow::Result<String> {
    let url = crate::api_client::chat_completions_url(base_url);
    info!("[TTS] 发送请求到 {} (chat_completions)", url);

    // 小米 MiMo TTS 的 voice 默认值
    let default_voice = "mimo_default";

    let mut request_body = serde_json::json!({
        "model": model,
        "messages": [
            {
                "role": "assistant",
                "content": text
            }
        ],
        "audio": {
            "voice": default_voice,
            "format": "mp3"
        },
        "modalities": ["text", "audio"]
    });

    merge_extra(&mut request_body, extra, "TTS");
    // tts_mode 是内部控制字段，不发送给 API
    if let serde_json::Value::Object(ref mut map) = request_body {
        map.remove("tts_mode");
    }

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

    // 尝试解析为 Chat Completions TTS 响应
    let chat_response: ChatTtsResponse = response.json().await?;

    let b64 = chat_response
        .choices
        .first()
        .and_then(|c| c.message.audio.as_ref())
        .and_then(|a| a.data.clone())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "TTS chat_completions 响应中未找到音频数据 (choices[0].message.audio.data)"
            )
        })?;

    info!("[TTS] 收到音频数据 (chat_completions), base64长度={}", b64.len());
    Ok(b64)
}

/// 发送请求并提取二进制音频响应，base64 编码后返回。
async fn send_and_extract_binary(
    client: &Client,
    url: &str,
    api_key: &str,
    request_body: &serde_json::Value,
    tag: &str,
) -> anyhow::Result<String> {
    let mut req = client.post(url).json(request_body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    } else {
        warn!("[{}] API Key 为空", tag);
    }

    let response = req.send().await?;
    let status = response.status();

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        error!("[{}] API 错误 ({}): {}", tag, status, body);
        anyhow::bail!("{} API error ({}): {}", tag, status, body);
    }

    let bytes = response.bytes().await?;
    info!("[{}] 收到音频数据, 大小={}bytes", tag, bytes.len());

    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(b64)
}
