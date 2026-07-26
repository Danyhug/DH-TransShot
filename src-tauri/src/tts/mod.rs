use crate::config::merge_extra;
use base64::Engine;
use log::{error, info, warn};
use reqwest::Client;
use serde::Deserialize;

/// chat+audio 协议默认音色（如小米 MiMo），可通过 `extra.voice` 覆盖。
/// 注意：这是「裸名字」音色（如 `Chloe`），不同于 audio/speech 协议的 `{model}:alex` 形态。
const DEFAULT_CHAT_AUDIO_VOICE: &str = "Chloe";
/// chat+audio 协议默认音频格式（小米文档示例为 `wav`），可通过 `extra.format` 覆盖。
const DEFAULT_CHAT_AUDIO_FORMAT: &str = "wav";
/// chat+audio 协议默认风格指令（作为 `user` 消息），可通过 `extra.style` 覆盖为空或自定义。
const DEFAULT_CHAT_AUDIO_STYLE: &str = "用自然、平稳、清晰的语气朗读。";

/// Resolve the TTS endpoint URL from a base_url.
///
/// 复用 `api_client::build_endpoint_url` 的自适应拼接规则（根/版本段/完整端点/`#` raw）。
/// 当用户用 `#` raw 或完整路径把 base_url 指向 `.../chat/completions` 时，这里原样返回该
/// chat 端点，随后由 [`is_chat_audio_endpoint`] 判定改走小米式 chat+audio 协议。
fn resolve_tts_endpoint_url(base_url: &str) -> String {
    crate::api_client::build_endpoint_url(base_url, "audio/speech")
}

/// 判断解析出的端点是否为 Chat Completions（→ 走小米式 chat+audio 协议）。
fn is_chat_audio_endpoint(url: &str) -> bool {
    url.split('?')
        .next()
        .unwrap_or(url)
        .trim_end_matches('/')
        .ends_with("chat/completions")
}

/// Call the configured TTS endpoint and return the audio as base64.
///
/// 按解析出的端点自适应选择协议：
/// - 端点以 `chat/completions` 结尾 → 小米式 **chat+audio** 协议
///   （`messages` 传风格指令+文本，`audio:{format,voice}` 指定音色，
///   响应 `choices[0].message.audio.data` 已是 base64）
/// - 其余 → OpenAI 兼容 **audio/speech** 协议
///   （`{model,input,voice,response_format}`，响应为二进制音频）
///
/// 两条路径最终都返回「base64 编码的音频字符串」。
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

    let url = resolve_tts_endpoint_url(base_url);

    if is_chat_audio_endpoint(&url) {
        synthesize_chat_audio(client, &url, api_key, model, extra, text).await
    } else {
        synthesize_audio_speech(client, &url, api_key, model, extra, text).await
    }
}

/// OpenAI 兼容 `/v1/audio/speech` 协议：请求体 `{model, input, voice, response_format}`，
/// 响应为二进制音频，读出后 base64 编码返回。`extra` 整包合并进请求体。
async fn synthesize_audio_speech(
    client: &Client,
    url: &str,
    api_key: &str,
    model: &str,
    extra: &str,
    text: &str,
) -> anyhow::Result<String> {
    info!(
        "[TTS] 协议=audio/speech, 发送请求到 {}, model={}, 文本长度={}",
        url,
        model,
        text.len()
    );

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

    let mut req = client.post(url).json(&request_body);
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

/// 小米 MiMo 式 **chat+audio** 协议：走 `/v1/chat/completions`。
///
/// 请求体 `{model, messages, audio:{format, voice}}`，其中 `messages` 按官方示例组织为
/// `user`（风格指令）+ `assistant`（要朗读的文本）；响应中 `choices[0].message.audio.data`
/// 已是 base64 音频，直接返回。
///
/// 说明：该协议**不整包合并 `extra`**（其 audio/speech 字段如 `response_format`/`speed`/
/// `sample_rate` 会污染甚至报错 chat 请求体），仅解释以下三个键：
/// - `voice`：音色裸名字（默认 `Chloe`）；忽略含 `/` 的 model-scoped 音色（那是另一协议遗留）
/// - `format`：音频格式（默认 `wav`）
/// - `style`：风格指令 / `user` 消息内容（默认中性提示；置为空串则不发送 `user` 消息）
async fn synthesize_chat_audio(
    client: &Client,
    url: &str,
    api_key: &str,
    model: &str,
    extra: &str,
    text: &str,
) -> anyhow::Result<String> {
    let extra_json: serde_json::Value =
        serde_json::from_str(extra.trim()).unwrap_or(serde_json::Value::Null);

    // voice：忽略 audio/speech 式的 `model:name`（含 '/' 的 model-scoped 音色对 chat+audio 无效）
    let voice = extra_json
        .get("voice")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty() && !v.contains('/'))
        .unwrap_or(DEFAULT_CHAT_AUDIO_VOICE);

    // format：仅认 chat+audio 专用的 `format` 键；`response_format` 属于另一协议，不复用
    let format = extra_json
        .get("format")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(DEFAULT_CHAT_AUDIO_FORMAT);

    // style：风格指令（user 消息），可用 extra.style 覆盖；置为空串则不发送 user 消息
    let style = extra_json
        .get("style")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or(DEFAULT_CHAT_AUDIO_STYLE);

    info!(
        "[TTS] 协议=chat+audio, 发送请求到 {}, model={}, voice={}, format={}, 文本长度={}",
        url,
        model,
        voice,
        format,
        text.len()
    );

    // 官方示例：user=风格指令，assistant=要朗读的文本
    let mut messages = Vec::new();
    if !style.is_empty() {
        messages.push(serde_json::json!({ "role": "user", "content": style }));
    }
    messages.push(serde_json::json!({ "role": "assistant", "content": text }));

    let request_body = serde_json::json!({
        "model": model,
        "messages": messages,
        "audio": { "format": format, "voice": voice }
    });

    let mut req = client.post(url).json(&request_body);
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

    let parsed: ChatAudioResponse = response.json().await?;
    let b64 = parsed
        .choices
        .into_iter()
        .next()
        .and_then(|c| c.message.audio)
        .map(|a| a.data)
        .ok_or_else(|| {
            anyhow::anyhow!("TTS 响应中未找到音频数据 (choices[0].message.audio.data)")
        })?;

    info!("[TTS] 收到音频数据(base64), 长度={}", b64.len());
    Ok(b64)
}

/// chat+audio 协议的响应结构（仅取所需字段，未知字段忽略）。
#[derive(Deserialize)]
struct ChatAudioResponse {
    choices: Vec<ChatAudioChoice>,
}

#[derive(Deserialize)]
struct ChatAudioChoice {
    message: ChatAudioMessage,
}

#[derive(Deserialize)]
struct ChatAudioMessage {
    audio: Option<ChatAudioData>,
}

#[derive(Deserialize)]
struct ChatAudioData {
    /// base64 编码的音频数据。
    data: String,
}
