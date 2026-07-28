use crate::config::merge_extra;
use base64::Engine;
use futures_util::StreamExt;
use log::{error, info, warn};
use reqwest::Client;
use serde::Deserialize;
use std::time::Instant;

/// chat+audio 协议默认音色（如小米 MiMo），可通过 `extra.voice` 覆盖。
/// 注意：这是「裸名字」音色（如 `Milo`），不同于 audio/speech 协议的 `{model}:alex` 形态。
const DEFAULT_CHAT_AUDIO_VOICE: &str = "Milo";
/// chat+audio 协议**非流式**默认音频格式（小米文档示例为 `wav`），可通过 `extra.format` 覆盖。
/// 流式路径固定 `pcm16`（官方要求，否则分块无法拼接成完整音频）。
const DEFAULT_CHAT_AUDIO_FORMAT: &str = "wav";
/// chat+audio 协议默认风格指令（作为 `user` 消息），可通过 `extra.style` 覆盖为空或自定义。
const DEFAULT_CHAT_AUDIO_STYLE: &str = "用自然、平稳、清晰的语气朗读。";
/// chat+audio 流式分块的 PCM 参数（小米文档：24kHz / 单声道 / PCM16LE）。
const CHAT_AUDIO_STREAM_SAMPLE_RATE: u32 = 24000;
const CHAT_AUDIO_STREAM_CHANNELS: u16 = 1;

/// 合成结果：`audio_base64` 始终是「可直接播放的完整音频」（base64）。
pub struct SpeechResult {
    /// 完整音频的 base64（流式路径为拼接后的 WAV）
    pub audio_base64: String,
    /// 已推送给前端的流式分块数量；0 表示本次没有走流式分块
    pub chunk_count: usize,
    /// 流式分块的采样率（Hz），非流式为 0
    pub sample_rate: u32,
}

impl SpeechResult {
    fn whole(audio_base64: String) -> Self {
        Self {
            audio_base64,
            chunk_count: 0,
            sample_rate: 0,
        }
    }
}

/// 流式分块回调：`(分块序号, base64 PCM16 分块)`，由命令层转成 Tauri 事件推给前端。
pub type ChunkSink<'a> = &'a (dyn Fn(usize, &str) + Send + Sync);

/// chat+audio 流式分块的采样率（Hz），供命令层填入事件负载。
pub fn stream_sample_rate() -> u32 {
    CHAT_AUDIO_STREAM_SAMPLE_RATE
}

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
///   （`messages` 传风格指令+文本，`audio:{format,voice}` 指定音色）
///   - 默认走 **SSE 流式**（`stream:true` + `pcm16`）：分块经 `on_chunk` 实时回传，
///     结束后把所有 PCM 拼成 WAV 返回；可用 `extra.stream=false` 关掉
/// - 其余 → OpenAI 兼容 **audio/speech** 协议
///   （`{model,input,voice,response_format}`，响应为二进制音频）
///
/// 两条路径最终都返回「base64 编码的完整音频」。
pub async fn synthesize(
    client: &Client,
    base_url: &str,
    api_key: &str,
    model: &str,
    extra: &str,
    text: &str,
    on_chunk: Option<ChunkSink<'_>>,
) -> anyhow::Result<SpeechResult> {
    if base_url.trim().is_empty() {
        anyhow::bail!("TTS 未配置 API 地址，请在设置中填写 base_url");
    }

    let url = resolve_tts_endpoint_url(base_url);

    if is_chat_audio_endpoint(&url) {
        synthesize_chat_audio(client, &url, api_key, model, extra, text, on_chunk).await
    } else {
        synthesize_audio_speech(client, &url, api_key, model, extra, text)
            .await
            .map(SpeechResult::whole)
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

/// chat+audio 协议从 `extra` 解释出来的参数（该协议**不整包合并 `extra`**，
/// 因为 audio/speech 的 `response_format`/`speed`/`sample_rate` 会污染甚至报错 chat 请求体）。
struct ChatAudioOptions {
    /// 音色裸名字（默认 [`DEFAULT_CHAT_AUDIO_VOICE`]）；忽略含 `/` 的 model-scoped 音色
    voice: String,
    /// 非流式音频格式（默认 [`DEFAULT_CHAT_AUDIO_FORMAT`]）
    format: String,
    /// 风格指令 / `user` 消息内容（默认中性提示；置为空串则不发送 `user` 消息）
    style: String,
    /// 是否走 SSE 流式（默认 true，可用 `extra.stream=false` 关掉）
    stream: bool,
}

fn parse_chat_audio_options(extra: &str) -> ChatAudioOptions {
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

    let stream = extra_json
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    ChatAudioOptions {
        voice: voice.to_string(),
        format: format.to_string(),
        style: style.to_string(),
        stream,
    }
}

/// 组织 chat+audio 的 `messages`（官方约定）：`user`=风格指令，`assistant`=要朗读的文本。
fn build_chat_audio_messages(style: &str, text: &str) -> Vec<serde_json::Value> {
    let mut messages = Vec::new();
    if !style.is_empty() {
        messages.push(serde_json::json!({ "role": "user", "content": style }));
    }
    messages.push(serde_json::json!({ "role": "assistant", "content": text }));
    messages
}

/// 小米 MiMo 式 **chat+audio** 协议入口：默认流式，失败时回退非流式。
async fn synthesize_chat_audio(
    client: &Client,
    url: &str,
    api_key: &str,
    model: &str,
    extra: &str,
    text: &str,
    on_chunk: Option<ChunkSink<'_>>,
) -> anyhow::Result<SpeechResult> {
    let opts = parse_chat_audio_options(extra);

    if opts.stream {
        match synthesize_chat_audio_stream(client, url, api_key, model, &opts, text, on_chunk).await
        {
            Ok(result) => return Ok(result),
            Err(e) => warn!("[TTS] 流式合成失败，回退非流式: {}", e),
        }
    }

    synthesize_chat_audio_once(client, url, api_key, model, &opts, text)
        .await
        .map(SpeechResult::whole)
}

/// chat+audio 非流式：请求体 `{model, messages, audio:{format, voice}}`，
/// 响应中 `choices[0].message.audio.data` 已是 base64 音频，直接返回。
async fn synthesize_chat_audio_once(
    client: &Client,
    url: &str,
    api_key: &str,
    model: &str,
    opts: &ChatAudioOptions,
    text: &str,
) -> anyhow::Result<String> {
    info!(
        "[TTS] 协议=chat+audio(非流式), 发送请求到 {}, model={}, voice={}, format={}, 文本长度={}",
        url,
        model,
        opts.voice,
        opts.format,
        text.len()
    );

    let request_body = serde_json::json!({
        "model": model,
        "messages": build_chat_audio_messages(&opts.style, text),
        "audio": { "format": opts.format, "voice": opts.voice }
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
    let b64 = parsed.first_audio_data().ok_or_else(|| {
        anyhow::anyhow!("TTS 响应中未找到音频数据 (choices[0].message.audio.data)")
    })?;

    info!("[TTS] 收到音频数据(base64), 长度={}", b64.len());
    Ok(b64)
}

/// chat+audio 流式：请求体额外带 `stream:true`，音频格式固定 `pcm16`（官方要求，
/// 只有裸 PCM 分块才能直接拼接）。
///
/// 逐块解析 SSE（`data: {...}` 行），取 `choices[0].delta.audio.data`（base64 PCM16LE）：
/// - 每块通过 `on_chunk` 实时回传（命令层转 Tauri 事件 → 前端边收边播）
/// - 同时累积 PCM，结束后套 WAV 头返回完整音频（供缓存与重播）
async fn synthesize_chat_audio_stream(
    client: &Client,
    url: &str,
    api_key: &str,
    model: &str,
    opts: &ChatAudioOptions,
    text: &str,
    on_chunk: Option<ChunkSink<'_>>,
) -> anyhow::Result<SpeechResult> {
    info!(
        "[TTS] 协议=chat+audio(流式), 发送请求到 {}, model={}, voice={}, format=pcm16, 文本长度={}",
        url,
        model,
        opts.voice,
        text.len()
    );

    let request_body = serde_json::json!({
        "model": model,
        "messages": build_chat_audio_messages(&opts.style, text),
        "audio": { "format": "pcm16", "voice": opts.voice },
        "stream": true
    });

    let mut req = client.post(url).json(&request_body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    } else {
        warn!("[TTS] API Key 为空");
    }

    let started = Instant::now();
    let response = req.send().await?;
    let status = response.status();

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        error!("[TTS] 流式 API 错误 ({}): {}", status, body);
        anyhow::bail!("TTS API error ({}): {}", status, body);
    }

    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut pcm: Vec<u8> = Vec::new();
    let mut chunk_count = 0usize;
    let mut first_chunk_ms: Option<u128> = None;
    let mut saw_sse = false;

    while let Some(item) = stream.next().await {
        buffer.extend_from_slice(&item?);

        // 逐行消费缓冲区；不完整的尾部留到下一次 chunk
        while let Some(pos) = buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim();
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            saw_sse = true;
            let payload = payload.trim();
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
                warn!("[TTS] 流式分块 JSON 解析失败，已跳过");
                continue;
            };
            if let Some(err) = value.get("error") {
                anyhow::bail!("TTS 流式响应返回错误: {}", err);
            }
            let Some(data) = extract_stream_audio_data(&value) else {
                continue;
            };
            match base64::engine::general_purpose::STANDARD.decode(data) {
                Ok(bytes) => {
                    pcm.extend_from_slice(&bytes);
                    if let Some(sink) = on_chunk {
                        sink(chunk_count, data);
                    }
                    if first_chunk_ms.is_none() {
                        first_chunk_ms = Some(started.elapsed().as_millis());
                    }
                    chunk_count += 1;
                }
                Err(e) => warn!("[TTS] 流式分块 base64 解码失败: {}", e),
            }
        }
    }

    // 服务端没按 SSE 返回（不支持 stream 的兼容端点）：整体当普通 JSON 响应解析
    if !saw_sse {
        let parsed: ChatAudioResponse = serde_json::from_slice(&buffer)?;
        let b64 = parsed
            .first_audio_data()
            .ok_or_else(|| anyhow::anyhow!("流式响应既非 SSE 也不含音频数据"))?;
        info!(
            "[TTS] 服务端未走 SSE，按非流式响应处理, base64长度={}",
            b64.len()
        );
        return Ok(SpeechResult::whole(b64));
    }

    if pcm.is_empty() {
        anyhow::bail!("TTS 流式响应未返回任何音频分块");
    }

    let wav = wrap_pcm16_wav(
        &pcm,
        CHAT_AUDIO_STREAM_SAMPLE_RATE,
        CHAT_AUDIO_STREAM_CHANNELS,
    );
    info!(
        "[TTS] 流式合成完成, 分块数={}, PCM={}bytes, 首块耗时={}ms, 总耗时={}ms",
        chunk_count,
        pcm.len(),
        first_chunk_ms.unwrap_or(0),
        started.elapsed().as_millis()
    );

    Ok(SpeechResult {
        audio_base64: base64::engine::general_purpose::STANDARD.encode(&wav),
        chunk_count,
        sample_rate: CHAT_AUDIO_STREAM_SAMPLE_RATE,
    })
}

/// 从一条 SSE 分块里取音频 base64：优先 `choices[0].delta.audio.data`，
/// 兼容部分服务端在最后一块用 `choices[0].message.audio.data` 的写法。
fn extract_stream_audio_data(value: &serde_json::Value) -> Option<&str> {
    let choice = value.get("choices")?.get(0)?;
    for key in ["delta", "message"] {
        if let Some(data) = choice
            .get(key)
            .and_then(|m| m.get("audio"))
            .and_then(|a| a.get("data"))
            .and_then(|d| d.as_str())
        {
            if !data.is_empty() {
                return Some(data);
            }
        }
    }
    None
}

/// 给裸 PCM16LE 数据套一个 44 字节 WAV 头，得到可直接播放的完整音频。
fn wrap_pcm16_wav(pcm: &[u8], sample_rate: u32, channels: u16) -> Vec<u8> {
    const BITS_PER_SAMPLE: u16 = 16;
    let byte_rate = sample_rate * channels as u32 * (BITS_PER_SAMPLE / 8) as u32;
    let block_align = channels * (BITS_PER_SAMPLE / 8);
    let data_len = pcm.len() as u32;

    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk 长度
    wav.extend_from_slice(&1u16.to_le_bytes()); // 1 = PCM
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(pcm);
    wav
}

/// chat+audio 非流式响应结构（仅取所需字段，未知字段忽略）。
#[derive(Deserialize)]
struct ChatAudioResponse {
    choices: Vec<ChatAudioChoice>,
}

impl ChatAudioResponse {
    fn first_audio_data(self) -> Option<String> {
        self.choices
            .into_iter()
            .next()
            .and_then(|c| c.message.audio)
            .map(|a| a.data)
            .filter(|d| !d.is_empty())
    }
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
