use crate::config::AppState;
use log::{error, info, warn};
use serde::Serialize;
use tauri::{Emitter, State};

fn normalize_tts_text(text: &str) -> String {
    text.trim().replace("\r\n", "\n")
}

fn tts_cache_key(base_url: &str, model: &str, extra: &str, text: &str) -> String {
    format!("{base_url}\n{model}\n{extra}\n{text}")
}

/// 流式分块事件负载（事件名 `tts-chunk`）：前端按 `sessionId` 过滤后边收边播。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TtsChunkEvent<'a> {
    session_id: &'a str,
    /// 分块序号，从 0 开始
    seq: usize,
    /// base64 编码的 PCM16LE 单声道分块
    data: &'a str,
    sample_rate: u32,
}

/// 语音合成结果（流式命令用）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechResponse {
    /// 完整音频 base64（流式为拼接后的 WAV）
    pub audio: String,
    /// 本次推送的流式分块数量；0 表示前端应直接播放 `audio`
    pub chunk_count: usize,
    /// 流式分块采样率（Hz），非流式为 0
    pub sample_rate: u32,
}

/// Synthesize speech from text using the configured TTS service.
/// Returns base64-encoded audio data.
#[tauri::command]
pub async fn synthesize_speech(state: State<'_, AppState>, text: String) -> Result<String, String> {
    synthesize_inner(&state, &text, None).await.map(|r| r.audio)
}

/// 边收边播版本：与 [`synthesize_speech`] 相同的合成流程，但 chat+audio 流式分块会通过
/// `tts-chunk` 事件实时推给前端（`session_id` 用于区分并发/过期的朗读会话）。
///
/// 返回值里的 `audio` 仍是完整音频，前端用于写缓存；`chunk_count == 0` 时说明本次没有分块
/// （命中缓存 / 非 chat+audio 协议 / 服务端不支持流式），前端直接播放 `audio` 即可。
#[tauri::command]
pub async fn synthesize_speech_stream(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    text: String,
    session_id: String,
) -> Result<SpeechResponse, String> {
    let sink = {
        let app = app.clone();
        let session_id = session_id.clone();
        move |seq: usize, data: &str| {
            if let Err(e) = app.emit(
                "tts-chunk",
                TtsChunkEvent {
                    session_id: &session_id,
                    seq,
                    data,
                    sample_rate: crate::tts::stream_sample_rate(),
                },
            ) {
                warn!("[TTS] 推送流式分块失败: {}", e);
            }
        }
    };
    synthesize_inner(&state, &text, Some(&sink)).await
}

/// 共享的合成流程：规范化文本 → 解析当前生效配置 → 查缓存 → 合成 → 写缓存。
async fn synthesize_inner(
    state: &State<'_, AppState>,
    text: &str,
    on_chunk: Option<crate::tts::ChunkSink<'_>>,
) -> Result<SpeechResponse, String> {
    let normalized_text = normalize_tts_text(text);
    info!(
        "[TTS] synthesize_speech 开始, 原始文本长度={}, 规范化后长度={}, 边收边播={}",
        text.len(),
        normalized_text.len(),
        on_chunk.is_some()
    );

    let (base_url, api_key, model, extra) = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        let (b, k, m) = settings.tts.resolved(&settings.base_url, &settings.api_key);
        (b, k, m, settings.tts.extra.clone())
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
        return Ok(SpeechResponse {
            audio: cached,
            chunk_count: 0,
            sample_rate: 0,
        });
    }
    info!("[TTS] 缓存未命中，发起语音合成");

    let result = crate::tts::synthesize(
        &client,
        &base_url,
        &api_key,
        &model,
        &extra,
        &normalized_text,
        on_chunk,
    )
    .await
    .map_err(|e| e.to_string());

    match result {
        Ok(speech) => {
            info!(
                "[TTS] 语音合成完成, base64长度={}, 分块数={}",
                speech.audio_base64.len(),
                speech.chunk_count
            );
            state
                .tts_cache
                .lock()
                .map_err(|e| e.to_string())?
                .insert(cache_key, speech.audio_base64.clone());
            info!("[TTS] 已写入缓存");
            Ok(SpeechResponse {
                audio: speech.audio_base64,
                chunk_count: speech.chunk_count,
                sample_rate: speech.sample_rate,
            })
        }
        Err(e) => {
            error!("[TTS] 语音合成失败: {}", e);
            Err(e)
        }
    }
}
