use crate::config::AppState;
use log::{error, info, warn};
use serde::Serialize;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

fn normalize_tts_text(text: &str) -> String {
    text.trim().replace("\r\n", "\n")
}

fn tts_cache_key(base_url: &str, model: &str, extra: &str, text: &str) -> String {
    format!("{base_url}\n{model}\n{extra}\n{text}")
}

/// 流式通道上的**控制消息**（JSON）。音频分块不走这里，而是以
/// [`InvokeResponseBody::Raw`] 二进制形式直接发送（前端收到 `ArrayBuffer`）。
///
/// 通道消息由 Tauri 保证按发送顺序投递，因此 `End` 一定在所有分块之后到达，
/// 前端可据此判定「上游已结束」，无需依赖命令返回值与事件的先后。
#[derive(Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
enum TtsStreamMessage {
    /// 分块格式，必须在第一个分块之前发送
    #[serde(rename_all = "camelCase")]
    Start { sample_rate: u32, channels: u16 },
    /// 上游流已结束，共推送 `chunk_count` 个分块
    #[serde(rename_all = "camelCase")]
    End { chunk_count: usize },
}

fn send_stream_message(channel: &Channel<InvokeResponseBody>, message: &TtsStreamMessage) {
    match serde_json::to_string(message) {
        Ok(json) => {
            if let Err(e) = channel.send(InvokeResponseBody::Json(json)) {
                warn!("[TTS] 推送流式控制消息失败: {}", e);
            }
        }
        Err(e) => warn!("[TTS] 序列化流式控制消息失败: {}", e),
    }
}

/// 语音合成结果（流式命令用）。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechResponse {
    /// 完整音频 base64。**走了流式分块时为空串**：音频已通过通道逐块送达，
    /// 再回传一份完整 WAV 会让长文本多传输一遍数 MB 数据。
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
/// `on_chunk` 这个 IPC Channel 以二进制实时推给前端。
///
/// 相比全局事件（`app.emit`），Channel 只投递给发起调用的 webview，且大负载走 IPC
/// 自定义协议（fetch）而非把整段数据拼进 `eval` 字符串——长文本几百个分块时，后者会把主线程
/// 堵死，表现为「必须等流传完才开始播 / 长文本干脆播不出来」。
///
/// 返回值里的 `chunk_count == 0` 表示本次没有分块（命中缓存 / 非 chat+audio 协议 /
/// 服务端不支持流式），此时 `audio` 是完整音频，前端直接播放即可。
#[tauri::command]
pub async fn synthesize_speech_stream(
    state: State<'_, AppState>,
    text: String,
    on_chunk: Channel<InvokeResponseBody>,
) -> Result<SpeechResponse, String> {
    send_stream_message(
        &on_chunk,
        &TtsStreamMessage::Start {
            sample_rate: crate::tts::stream_sample_rate(),
            channels: crate::tts::stream_channels(),
        },
    );

    let sink = {
        let channel = on_chunk.clone();
        move |pcm: &[u8]| {
            if let Err(e) = channel.send(InvokeResponseBody::Raw(pcm.to_vec())) {
                warn!("[TTS] 推送流式分块失败: {}", e);
            }
        }
    };

    let mut result = synthesize_inner(&state, &text, Some(&sink)).await;

    let chunk_count = result.as_ref().map(|r| r.chunk_count).unwrap_or(0);
    send_stream_message(&on_chunk, &TtsStreamMessage::End { chunk_count });

    // 分块已经逐块送达前端，无需再回传一份完整音频（重播时走后端缓存即可）
    if let Ok(response) = result.as_mut() {
        if response.chunk_count > 0 {
            response.audio.clear();
        }
    }
    result
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
        settings.tts.resolved(&settings.base_url, &settings.api_key)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 控制消息是前后端的线上契约（前端按 `event` 字段分派），改动需同步 `src/lib/invoke.ts`
    #[test]
    fn stream_message_wire_format() {
        assert_eq!(
            serde_json::to_string(&TtsStreamMessage::Start {
                sample_rate: 24000,
                channels: 1
            })
            .unwrap(),
            r#"{"event":"start","sampleRate":24000,"channels":1}"#
        );
        assert_eq!(
            serde_json::to_string(&TtsStreamMessage::End { chunk_count: 7 }).unwrap(),
            r#"{"event":"end","chunkCount":7}"#
        );
    }
}
