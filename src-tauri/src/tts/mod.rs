use crate::config::merge_extra;
use base64::Engine;
use futures_util::StreamExt;
use log::{error, info, warn};
use reqwest::Client;
use serde::Deserialize;
use std::time::Instant;

/// chat+audio 协议默认音色（如小米 MiMo），可通过 `extra.voice` 覆盖。
/// 注意：这是「裸名字」音色（如 `Milo`），不同于 audio/speech 协议的 `{model}:alex` 形态。
///
/// 用 `mimo_default`（跟随集群：中国区=冰糖中文女声）而不是写死某个音色：本工具主要朗读
/// 中译文，默认落到英文男声 `Milo` 读中文效果很差。
const DEFAULT_CHAT_AUDIO_VOICE: &str = "mimo_default";
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

/// 手写 Debug 而不是 derive：`audio_base64` 动辄几 MB，derive 出来的实现会把整段音频
/// 打进日志/panic 信息，只暴露长度就够定位问题了。
impl std::fmt::Debug for SpeechResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeechResult")
            .field("audio_base64_len", &self.audio_base64.len())
            .field("chunk_count", &self.chunk_count)
            .field("sample_rate", &self.sample_rate)
            .finish()
    }
}

/// 流式分块回调：参数是**已解码的 PCM16LE 裸字节**（不是 base64）。
/// 命令层把它原样写进 IPC Channel 的二进制消息，避免 base64 膨胀与 JSON 转义。
pub type ChunkSink<'a> = &'a (dyn Fn(&[u8]) + Send + Sync);

/// chat+audio 流式分块的采样率（Hz），供命令层告知前端。
pub fn stream_sample_rate() -> u32 {
    CHAT_AUDIO_STREAM_SAMPLE_RATE
}

/// chat+audio 流式分块的声道数，供命令层告知前端。
pub fn stream_channels() -> u16 {
    CHAT_AUDIO_STREAM_CHANNELS
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
    let configured_voice = extra_json
        .get("voice")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty());
    if let Some(v) = configured_voice.filter(|v| v.contains('/')) {
        warn!(
            "[TTS] extra.voice=\"{}\" 是 audio/speech 协议的 model-scoped 音色，对 chat+audio 无效，\
             已回退默认音色 {}；请在该提供商的自定义参数里填裸音色名（如 冰糖 / Milo）",
            v, DEFAULT_CHAT_AUDIO_VOICE
        );
    }
    let voice = configured_voice
        .filter(|v| !v.contains('/'))
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
///
/// 但**已经推给前端分块之后就不再回退**：那一半音频已经在用户扬声器上响过了，
/// 再合成一遍既多花一次钱、多等几秒，听感也是「读到一半跳回开头重读」。
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
        // accum 由这里持有：流中途失败时也能读到「已推送多少块」
        let mut accum = StreamAccum::new(on_chunk);
        match synthesize_chat_audio_stream(client, url, api_key, model, &opts, text, &mut accum)
            .await
        {
            Ok(result) => return Ok(result),
            Err(e) if accum.chunk_count > 0 => {
                error!(
                    "[TTS] 流式合成中断（已推送 {} 个分块），不回退非流式: {}",
                    accum.chunk_count, e
                );
                return Err(e.context(format!(
                    "流式合成中断（已播放 {} 个分块）",
                    accum.chunk_count
                )));
            }
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

/// chat+audio 流式解析的累积状态。
///
/// 由 [`synthesize_chat_audio`] 持有而非 [`synthesize_chat_audio_stream`] 内部自造：
/// 流中途失败时调用方要能读到 `chunk_count`（已经推给前端的块数），据此决定能否回退非流式。
struct StreamAccum<'a> {
    on_chunk: Option<ChunkSink<'a>>,
    /// 累积的完整 PCM16LE（结束后套 WAV 头，供缓存与重播）
    pcm: Vec<u8>,
    /// 已推送给前端的分块数
    chunk_count: usize,
    first_chunk_ms: Option<u128>,
    started: Instant,
}

impl<'a> StreamAccum<'a> {
    fn new(on_chunk: Option<ChunkSink<'a>>) -> Self {
        Self {
            on_chunk,
            pcm: Vec::new(),
            chunk_count: 0,
            first_chunk_ms: None,
            started: Instant::now(),
        }
    }

    /// 处理一条 SSE 行；返回该行**是否为 `data:` 行**（用于判定服务端是否真的走了 SSE）。
    fn handle_line(&mut self, line: &str) -> anyhow::Result<bool> {
        let Some(payload) = line.trim().strip_prefix("data:") else {
            return Ok(false);
        };
        let payload = payload.trim();
        if payload.is_empty() || payload == "[DONE]" {
            return Ok(true);
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
            warn!("[TTS] 流式分块 JSON 解析失败，已跳过");
            return Ok(true);
        };
        // 必须滤掉 null：字段存在但值为 null 时 `get` 返回 `Some(Value::Null)`，
        // 而不少 OpenAI 兼容网关每一帧都带 `"error": null`——不滤就会第一帧即 bail，
        // 表现为「边收边播开着却每次都要等整段合成完」
        if let Some(err) = value.get("error").filter(|e| !e.is_null()) {
            anyhow::bail!("TTS 流式响应返回错误: {}", err);
        }
        let Some(data) = extract_stream_audio_data(&value) else {
            return Ok(true);
        };
        match base64::engine::general_purpose::STANDARD.decode(data) {
            Ok(bytes) => {
                // 先推给前端再累积：边收边播的首帧延迟优先于本地缓冲
                if let Some(sink) = self.on_chunk {
                    sink(&bytes);
                }
                self.pcm.extend_from_slice(&bytes);
                if self.first_chunk_ms.is_none() {
                    self.first_chunk_ms = Some(self.started.elapsed().as_millis());
                }
                self.chunk_count += 1;
            }
            Err(e) => warn!("[TTS] 流式分块 base64 解码失败: {}", e),
        }
        Ok(true)
    }
}

/// chat+audio 流式：请求体额外带 `stream:true`，音频格式固定 `pcm16`（官方要求，
/// 只有裸 PCM 分块才能直接拼接）。
///
/// 逐块解析 SSE（`data: {...}` 行），取 `choices[0].delta.audio.data`（base64 PCM16LE）：
/// - 每块解码后通过 `accum.on_chunk` 实时回传（命令层写入 IPC Channel → 前端边收边播）
/// - 同时累积 PCM，结束后套 WAV 头返回完整音频（供缓存与重播）
async fn synthesize_chat_audio_stream(
    client: &Client,
    url: &str,
    api_key: &str,
    model: &str,
    opts: &ChatAudioOptions,
    text: &str,
    accum: &mut StreamAccum<'_>,
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

    let response = req.send().await?;
    let status = response.status();

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        error!("[TTS] 流式 API 错误 ({}): {}", status, body);
        anyhow::bail!("TTS API error ({}): {}", status, body);
    }

    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    // 服务端不走 SSE 时要把**整个**响应体当 JSON 解析。逐行消费会把所有非 `data:` 行
    // drain 掉丢弃（格式化过的 JSON 就是这样被切碎的），所以在确认是 SSE 之前另存一份
    // 完整原文；一旦确认走了 SSE 立即释放，不给长音频白占一份内存。
    let mut raw: Vec<u8> = Vec::new();
    let mut saw_sse = false;

    while let Some(item) = stream.next().await {
        let bytes = item?;
        buffer.extend_from_slice(&bytes);
        if !saw_sse {
            raw.extend_from_slice(&bytes);
        }

        // 逐行消费缓冲区；不完整的尾部留到下一次 chunk
        while let Some(pos) = buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = buffer.drain(..=pos).collect();
            if accum.handle_line(&String::from_utf8_lossy(&line))? && !saw_sse {
                saw_sse = true;
                raw = Vec::new();
            }
        }
    }

    // 收尾：末行可能不带换行符（服务端直接断流），不补这一手会丢掉最后一块音频
    if !buffer.is_empty() && accum.handle_line(&String::from_utf8_lossy(&buffer))? {
        saw_sse = true;
    }

    // 服务端没按 SSE 返回（不支持 stream 的兼容端点）：整体当普通 JSON 响应解析
    if !saw_sse {
        let parsed: ChatAudioResponse = serde_json::from_slice(&raw)?;
        let b64 = parsed
            .first_audio_data()
            .ok_or_else(|| anyhow::anyhow!("流式响应既非 SSE 也不含音频数据"))?;
        info!(
            "[TTS] 服务端未走 SSE，按非流式响应处理, base64长度={}",
            b64.len()
        );
        return Ok(SpeechResult::whole(b64));
    }

    if accum.pcm.is_empty() {
        anyhow::bail!("TTS 流式响应未返回任何音频分块");
    }

    let wav = wrap_pcm16_wav(
        &accum.pcm,
        CHAT_AUDIO_STREAM_SAMPLE_RATE,
        CHAT_AUDIO_STREAM_CHANNELS,
    );
    info!(
        "[TTS] 流式合成完成, 分块数={}, PCM={}bytes, 首块耗时={}ms, 总耗时={}ms",
        accum.chunk_count,
        accum.pcm.len(),
        accum.first_chunk_ms.unwrap_or(0),
        accum.started.elapsed().as_millis()
    );

    Ok(SpeechResult {
        audio_base64: base64::engine::general_purpose::STANDARD.encode(&wav),
        chunk_count: accum.chunk_count,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn data_line(b64: &str) -> String {
        format!(r#"data: {{"choices":[{{"delta":{{"audio":{{"data":"{b64}"}}}}}}]}}"#)
    }

    #[test]
    fn handle_line_collects_pcm() {
        let mut accum = StreamAccum::new(None);
        // "AAEC" -> [0x00, 0x01, 0x02]
        assert!(accum.handle_line(&data_line("AAEC")).unwrap());
        assert_eq!(accum.chunk_count, 1);
        assert_eq!(accum.pcm, vec![0x00, 0x01, 0x02]);
    }

    /// 非 `data:` 行不算 SSE —— `saw_sse` 靠这个返回值判定，判错会走到非流式兜底解析
    #[test]
    fn handle_line_reports_non_sse_lines() {
        let mut accum = StreamAccum::new(None);
        assert!(!accum.handle_line("").unwrap());
        assert!(!accum.handle_line("{").unwrap());
        assert!(!accum.handle_line(r#"  "choices": ["#).unwrap());
        assert!(accum.handle_line("data: [DONE]").unwrap());
        assert_eq!(accum.chunk_count, 0);
    }

    /// 网关常在每帧带 `"error": null`；`get()` 对它返回 `Some(Value::Null)`，
    /// 不滤掉就会第一帧即失败 → 流式形同虚设，每次都退回非流式
    #[test]
    fn handle_line_ignores_null_error_field() {
        let mut accum = StreamAccum::new(None);
        let line = r#"data: {"error":null,"choices":[{"delta":{"audio":{"data":"AAEC"}}}]}"#;
        assert!(accum.handle_line(line).unwrap());
        assert_eq!(accum.chunk_count, 1);
    }

    #[test]
    fn handle_line_fails_on_real_error_field() {
        let mut accum = StreamAccum::new(None);
        let line = r#"data: {"error":{"message":"quota exceeded"}}"#;
        assert!(accum.handle_line(line).is_err());
    }

    /// 末块 `message.audio.data` 的兼容写法
    #[test]
    fn extract_audio_prefers_delta_then_message() {
        let delta: serde_json::Value =
            serde_json::from_str(r#"{"choices":[{"delta":{"audio":{"data":"aa"}}}]}"#).unwrap();
        assert_eq!(extract_stream_audio_data(&delta), Some("aa"));
        let message: serde_json::Value =
            serde_json::from_str(r#"{"choices":[{"message":{"audio":{"data":"bb"}}}]}"#).unwrap();
        assert_eq!(extract_stream_audio_data(&message), Some("bb"));
    }

    #[test]
    fn wav_header_is_well_formed() {
        let wav = wrap_pcm16_wav(&[1, 2, 3, 4], 24000, 1);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 4);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 24000);
        assert_eq!(wav.len(), 48);
    }

    /// 跨端契约：前端 `src/lib/tts.ts` 的 `wrapPcm16Wav` 会在本地把流式 PCM 拼成 WAV
    /// 回填缓存，和后端这份写进同一个缓存语义里——两边字节必须一模一样。
    /// 改了任一侧的 WAV 头，这个用例会先炸。
    #[test]
    fn wav_bytes_match_frontend_implementation() {
        let wav = wrap_pcm16_wav(&[1, 2, 3, 4, 250, 251, 252, 253], 24000, 1);
        assert_eq!(
            base64::engine::general_purpose::STANDARD.encode(&wav),
            "UklGRiwAAABXQVZFZm10IBAAAAABAAEAwF0AAIC7AAACABAAZGF0YQgAAAABAgME+vv8/Q=="
        );
    }

    // ── 对着真实 HTTP 响应验流式解析 ──────────────────────────────────────
    // SSE 分帧、末行无换行、非 SSE 兜底这几条都发生在 HTTP 字节流层面，
    // 单测 handle_line 覆盖不到，必须起个假服务端跑一遍。

    /// 起一个只服务一次的假 HTTP 服务端，返回可直接喂给 `synthesize` 的 base_url
    /// （带 `#` raw 标记，避免被自适应规则拼成 audio/speech）。
    async fn spawn_once(body: &'static str, content_type: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            // 先把请求收干净再应答，否则对端可能拿到 RST 导致响应被截断
            let mut req = Vec::new();
            let mut buf = [0u8; 2048];
            loop {
                let n = sock.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                req.extend_from_slice(&buf[..n]);
                let Some(head_end) = req.windows(4).position(|w| w == b"\r\n\r\n") else {
                    continue;
                };
                let head = String::from_utf8_lossy(&req[..head_end]).to_lowercase();
                let want: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                if req.len() >= head_end + 4 + want {
                    break;
                }
            }
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            sock.write_all(head.as_bytes()).await.unwrap();
            sock.write_all(body.as_bytes()).await.unwrap();
            sock.flush().await.unwrap();
        });
        format!("http://{addr}/v1/chat/completions#")
    }

    /// 末帧不带换行符时（服务端直接断流）不能把最后一块音频丢掉
    #[tokio::test]
    async fn stream_keeps_last_frame_without_trailing_newline() {
        // "AAEC" -> 3 bytes, "AwQF" -> 3 bytes；第二帧后面**故意不带 \n**
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"audio\":{\"data\":\"AAEC\"}}}]}\n",
            "data: {\"choices\":[{\"delta\":{\"audio\":{\"data\":\"AwQF\"}}}]}"
        );
        let url = spawn_once(body, "text/event-stream").await;

        let pushed = std::sync::Mutex::new(Vec::<u8>::new());
        let sink = |b: &[u8]| pushed.lock().unwrap().extend_from_slice(b);
        let result = synthesize(&Client::new(), &url, "", "m", "", "hi", Some(&sink))
            .await
            .unwrap();

        assert_eq!(result.chunk_count, 2, "末帧无换行也要算进来");
        assert_eq!(result.sample_rate, CHAT_AUDIO_STREAM_SAMPLE_RATE);
        // 推给前端的是解码后的裸 PCM，且顺序与到达一致
        assert_eq!(*pushed.lock().unwrap(), vec![0, 1, 2, 3, 4, 5]);
        // 完整音频 = 44 字节 WAV 头 + 6 字节 PCM
        let wav = base64::engine::general_purpose::STANDARD
            .decode(&result.audio_base64)
            .unwrap();
        assert_eq!(wav.len(), 50);
        assert_eq!(&wav[0..4], b"RIFF");
    }

    /// 服务端不支持 stream 时会返回**普通 JSON**；若是格式化过的（带换行），
    /// 逐行消费会把它切碎，必须靠另存的完整原文兜底解析
    #[tokio::test]
    async fn stream_falls_back_to_pretty_printed_json() {
        let body = "{\n  \"choices\": [\n    {\n      \"message\": {\n        \"audio\": {\n          \"data\": \"QUJD\"\n        }\n      }\n    }\n  ]\n}\n";
        let url = spawn_once(body, "application/json").await;

        let result = synthesize(&Client::new(), &url, "", "m", "", "hi", None)
            .await
            .unwrap();

        assert_eq!(result.chunk_count, 0, "非 SSE 响应不算流式分块");
        assert_eq!(result.audio_base64, "QUJD");
    }

    /// 已经推给前端分块之后流才出错：不能回退非流式（会多合成一次并从头重播），
    /// 错误里要带上已播块数，前端据此停掉播放
    #[tokio::test]
    async fn stream_error_after_chunks_does_not_fall_back() {
        let body = concat!(
            "data: {\"choices\":[{\"delta\":{\"audio\":{\"data\":\"AAEC\"}}}]}\n",
            "data: {\"error\":{\"message\":\"upstream exploded\"}}\n"
        );
        let url = spawn_once(body, "text/event-stream").await;

        let pushed = std::sync::Mutex::new(0usize);
        let sink = |_: &[u8]| *pushed.lock().unwrap() += 1;
        let err = synthesize(&Client::new(), &url, "", "m", "", "hi", Some(&sink))
            .await
            .unwrap_err();

        assert_eq!(*pushed.lock().unwrap(), 1);
        assert!(
            format!("{err:#}").contains("已播放 1 个分块"),
            "错误里要带已播块数，实际: {err:#}"
        );
    }

    /// 一块都没推出去就失败，才允许回退非流式（此时假服务端已下线，回退请求必然失败，
    /// 错误信息不会带「已播放」字样——以此区分两条路径）
    #[tokio::test]
    async fn stream_error_before_any_chunk_falls_back() {
        let body = "data: {\"error\":{\"message\":\"bad voice\"}}\n";
        let url = spawn_once(body, "text/event-stream").await;

        let err = synthesize(&Client::new(), &url, "", "m", "", "hi", None)
            .await
            .unwrap_err();

        assert!(
            !format!("{err:#}").contains("已播放"),
            "没推过块就该走回退，实际: {err:#}"
        );
    }
}
