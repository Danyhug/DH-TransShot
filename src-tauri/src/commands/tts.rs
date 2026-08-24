use crate::audio::Playback;
use crate::config::AppState;
use crate::tts::StreamSink;
use base64::Engine;
use log::{error, info, warn};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::State;

/// 轮询「是否播完」的间隔。
///
/// 播放跑在 [`crate::audio`] 自己的输出线程上，这里只是等它跑完，顺带每轮检查有没有被
/// 后来的朗读抢占——被抢占时输出设备已经关了，死等队列播空会永远等不到。
const DRAIN_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 播放位置这么久没变就判定输出设备已经不在拉样本（拔耳机、设备被独占……）。
/// 没有这一层的话队列永远播不空，这次 invoke 会一直挂着、前端的加载态也一直转。
const PLAYBACK_STALL_TIMEOUT: Duration = Duration::from_secs(10);

fn normalize_tts_text(text: &str) -> String {
    text.trim().replace("\r\n", "\n")
}

fn tts_cache_key(base_url: &str, model: &str, extra: &str, text: &str) -> String {
    format!("{base_url}\n{model}\n{extra}\n{text}")
}

/// 播放状态控制消息（IPC Channel 上的 JSON）。
///
/// 音频本身**不再经过 IPC**——它由 Rust 侧直接送进输出设备，这里只剩「已经出声了」
/// 这一个信号，前端据此熄灭按钮的加载态。
#[derive(Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
enum TtsPlaybackMessage {
    /// 第一段音频已送入输出设备
    Start,
}

fn send_playback_message(channel: &Channel<InvokeResponseBody>, message: &TtsPlaybackMessage) {
    match serde_json::to_string(message) {
        Ok(json) => {
            if let Err(e) = channel.send(InvokeResponseBody::Json(json)) {
                warn!("[TTS] 推送播放状态失败: {}", e);
            }
        }
        Err(e) => warn!("[TTS] 序列化播放状态失败: {}", e),
    }
}

/// 合成结果（仅命令层内部使用，不再回传给前端）。
struct Synthesized {
    /// 完整音频 base64
    audio: String,
    /// 已边收边播出去的分块数；`0` 表示要整段播 `audio`
    chunk_count: usize,
}

/// 把流式分块直接送进本地音频输出。
struct PlaybackSink<'a> {
    audio: &'a crate::audio::AudioOutput,
    playback: &'a Playback,
    channel: &'a Channel<InvokeResponseBody>,
    started: AtomicBool,
}

impl StreamSink for PlaybackSink<'_> {
    fn chunk(&self, pcm: &[u8]) {
        // 已被后来的朗读抢占：设备都关了，再往队列里堆几 MB 没有意义
        if !self.audio.is_current(self.playback.generation()) {
            return;
        }
        self.playback.push_pcm16(pcm);
        if !self.started.swap(true, Ordering::SeqCst) {
            info!("[TTS] 首块已送入输出设备");
            send_playback_message(self.channel, &TtsPlaybackMessage::Start);
        }
    }

    fn discard(&self) {
        self.playback.discard();
    }
}

/// 朗读一段文本：合成 → 送进本地输出设备 → **播完才返回**。
///
/// 播放不经过 WebView（原因见 [`crate::audio`] 的模块注释：窗口一隐藏 WebKit 就会让
/// `AudioContext` 空转渲染，日志一切正常却一声不响）。因此这个命令只用 `on_event`
/// 回传「已经出声了」，音频数据一个字节都不过 IPC。
///
/// 调用即**抢占**上一段朗读；前端不必先调 [`stop_speech`]（两个 invoke 谁先到达没有保证，
/// 先停后播反而可能把新的这段停掉）。
#[tauri::command]
pub async fn speak_text(
    state: State<'_, AppState>,
    text: String,
    on_event: Channel<InvokeResponseBody>,
) -> Result<(), String> {
    let normalized = normalize_tts_text(&text);
    if normalized.is_empty() {
        return Ok(());
    }

    let stream_playback = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.speech.stream_playback
    };

    let playback = state
        .audio
        .begin(
            crate::tts::stream_sample_rate(),
            crate::tts::stream_channels(),
        )
        .map_err(|e| {
            error!("[TTS] 打开音频输出失败: {:#}", e);
            format!("{e:#}")
        })?;
    let generation = playback.generation();

    let sink = PlaybackSink {
        audio: &state.audio,
        playback: &playback,
        channel: &on_event,
        started: AtomicBool::new(false),
    };
    let on_chunk = if stream_playback {
        Some(&sink as _)
    } else {
        None
    };

    let synthesized = match synthesize_inner(&state, &normalized, on_chunk).await {
        Ok(synthesized) => synthesized,
        Err(e) => {
            // 合成失败时可能已经播出去一小段，收掉别让残声继续响
            state.audio.finish(generation);
            return Err(e);
        }
    };

    if !state.audio.is_current(generation) {
        info!("[TTS] 本次朗读已被抢占，不再播放");
        return Ok(());
    }

    if synthesized.chunk_count == 0 {
        // 命中缓存 / audio/speech 协议 / 服务端未流式 / 关掉了边收边播 → 整段播
        if synthesized.audio.is_empty() {
            warn!("[TTS] 合成结果为空，无可播放音频");
            state.audio.finish(generation);
            return Ok(());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&synthesized.audio)
            .map_err(|e| {
                state.audio.finish(generation);
                error!("[TTS] 音频 base64 解码失败: {}", e);
                format!("音频数据无法解码: {e}")
            })?;
        info!("[TTS] 整段播放, 音频={}bytes", bytes.len());
        if let Err(e) = playback.play_encoded(bytes) {
            state.audio.finish(generation);
            error!("[TTS] 播放失败: {:#}", e);
            return Err(format!("{e:#}"));
        }
        send_playback_message(&on_event, &TtsPlaybackMessage::Start);
    }

    // 等播完。两条退出路径：被抢占（设备已关，队列不会再播空）、播放停滞（设备掉了）
    let mut last_position = playback.position();
    let mut last_progress = Instant::now();
    while state.audio.is_current(generation) && !playback.is_drained() {
        tokio::time::sleep(DRAIN_POLL_INTERVAL).await;
        let position = playback.position();
        if position != last_position {
            last_position = position;
            last_progress = Instant::now();
        } else if last_progress.elapsed() >= PLAYBACK_STALL_TIMEOUT {
            warn!(
                "[TTS] 播放已停滞 {}s（输出设备可能已断开），提前收场",
                PLAYBACK_STALL_TIMEOUT.as_secs()
            );
            break;
        }
    }
    state.audio.finish(generation);
    info!("[TTS] 朗读结束 (session={})", generation);
    Ok(())
}

/// 停止当前朗读（对应前端的停止按钮）。
#[tauri::command]
pub fn stop_speech(state: State<'_, AppState>) {
    info!("[TTS] 停止朗读");
    state.audio.stop();
}

/// 共享的合成流程：规范化文本 → 解析当前生效配置 → 查缓存 → 合成 → 写缓存。
async fn synthesize_inner(
    state: &State<'_, AppState>,
    normalized_text: &str,
    on_chunk: Option<crate::tts::ChunkSink<'_>>,
) -> Result<Synthesized, String> {
    info!(
        "[TTS] 开始合成, 文本长度={}, 边收边播={}",
        normalized_text.len(),
        on_chunk.is_some()
    );

    let (base_url, api_key, model, extra) = {
        let settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.tts.resolved(&settings.base_url, &settings.api_key)
    };
    let client = state.http_client.clone();
    info!("[TTS] 使用 model={}, base_url={}", model, base_url);
    let cache_key = tts_cache_key(&base_url, &model, &extra, normalized_text);

    if let Some(cached) = state
        .tts_cache
        .lock()
        .map_err(|e| e.to_string())?
        .get(&cache_key)
    {
        info!("[TTS] 命中缓存, base64长度={}", cached.len());
        return Ok(Synthesized {
            audio: cached,
            chunk_count: 0,
        });
    }
    info!("[TTS] 缓存未命中，发起语音合成");

    let result = crate::tts::synthesize(
        &client,
        &base_url,
        &api_key,
        &model,
        &extra,
        normalized_text,
        on_chunk,
    )
    .await
    .map_err(|e| format!("{e:#}"));

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
            Ok(Synthesized {
                audio: speech.audio_base64,
                chunk_count: speech.chunk_count,
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
    fn playback_message_wire_format() {
        assert_eq!(
            serde_json::to_string(&TtsPlaybackMessage::Start).unwrap(),
            r#"{"event":"start"}"#
        );
    }

    /// 缓存键要覆盖影响音频输出的所有配置项，改了配置就该重新合成
    #[test]
    fn cache_key_covers_all_inputs() {
        let base = tts_cache_key("u", "m", "e", "t");
        assert_ne!(base, tts_cache_key("u2", "m", "e", "t"));
        assert_ne!(base, tts_cache_key("u", "m2", "e", "t"));
        assert_ne!(base, tts_cache_key("u", "m", "e2", "t"));
        assert_ne!(base, tts_cache_key("u", "m", "e", "t2"));
    }
}
