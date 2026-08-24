//! 本地音频输出（rodio / cpal）。
//!
//! # 为什么播放放在 Rust 侧而不是 WebView
//!
//! 主窗口失焦会自动隐藏（`App.tsx` 的 blur → `hide()`），而 WKWebView 一旦被标记为遮挡，
//! WebKit 就停掉 `AudioContext` 背后的音频单元，却仍用定时器时钟继续「空转渲染」：
//! `state` 是 `running`、`currentTime` 正常推进、`onended` 照常触发、样本却没送到输出设备。
//! JS 侧没有任何状态位能查出这种哑火——前端先后试过提前预热输出设备、每次朗读重建
//! `AudioContext`、比对墙上时钟与音频时长做兜底，都挡不住。
//!
//! 而快捷键翻译的典型流程恰恰是「窗口弹出 → 焦点回到原 App → 窗口隐藏」，朗读几乎每次
//! 都发生在窗口隐藏期间，于是表现为「日志一切正常（分块全部排入、耗时≈音频时长）但一声不响」。
//!
//! 挪到 Rust 侧之后音频完全不经过 WebView：窗口显示/隐藏与播放无关，PCM 也不必再走 IPC。
//! 另一个附带好处是 rodio 的样本是**被输出设备按需拉取**的，不像 Web Audio 那样按时间线
//! 排程——设备冷启动再慢也只是晚一点开始拉，绝不会把开头的音频吞掉。

use std::io::Cursor;
use std::num::NonZero;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use anyhow::Context;
use log::info;
use rodio::buffer::SamplesBuffer;
use rodio::{Decoder, DeviceSinkBuilder, Player, Sample};

/// 锁中毒后照常拿数据：音频状态全是「当前播的是哪一段」这种可重建的信息，
/// 为它把整个朗读功能 panic 掉不值当。
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 输出设备线程的停机信号。
#[derive(Default)]
struct DeviceShutdown {
    closed: Mutex<bool>,
    cv: Condvar,
}

/// 输出设备句柄。
///
/// rodio 的 `MixerDeviceSink` 内含 `cpal::Stream`（`!Send`），只能待在创建它的线程上，
/// 所以这里持有的只是个停机信号：**drop 即通知那个线程退出，设备随之关闭**。
struct DeviceHandle {
    shutdown: Arc<DeviceShutdown>,
}

impl Drop for DeviceHandle {
    fn drop(&mut self) {
        *lock(&self.shutdown.closed) = true;
        self.shutdown.cv.notify_all();
    }
}

/// 打开默认输出设备，返回「可跨线程使用的播放队列」和「设备句柄」。
///
/// 每次朗读都重新打开而不是全程复用一个设备：cpal 的输出流绑定的是**打开时**的那个设备，
/// 用户中途连上蓝牙耳机 / 拔掉外接音箱时不会自动跟随，复用就会播到已经不用的设备上去。
/// 打开设备的开销被合成请求的 1s+ 网络往返吸收，听感上不增加起播延迟。
fn open_device() -> anyhow::Result<(Arc<Player>, DeviceHandle)> {
    let (ready_tx, ready_rx) = mpsc::channel::<anyhow::Result<Player>>();
    let shutdown = Arc::new(DeviceShutdown::default());
    let thread_shutdown = shutdown.clone();

    std::thread::Builder::new()
        .name("dh-tts-audio".into())
        .spawn(move || {
            let mut device = match DeviceSinkBuilder::open_default_sink() {
                Ok(device) => device,
                Err(e) => {
                    let _ = ready_tx.send(Err(anyhow::anyhow!("打开音频输出设备失败: {e}")));
                    return;
                }
            };
            // rodio 默认在 drop 时往 stderr 打一行提示，这里是正常的按次开关设备
            device.log_on_drop(false);
            let player = Player::connect_new(device.mixer());
            if ready_tx.send(Ok(player)).is_err() {
                return; // 调用方已经不要了
            }
            // 守着设备直到 DeviceHandle 被 drop；device 在这之后随作用域结束而关闭
            let mut closed = lock(&thread_shutdown.closed);
            while !*closed {
                closed = thread_shutdown
                    .cv
                    .wait(closed)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        })
        .context("启动音频输出线程失败")?;

    let player = ready_rx.recv().context("音频输出线程未返回结果")??;
    Ok((Arc::new(player), DeviceHandle { shutdown }))
}

/// 正在播放的那一段。
struct Active {
    generation: u64,
    player: Arc<Player>,
    /// drop 即关闭输出设备
    _device: DeviceHandle,
}

/// 进程级音频输出：同一时刻只播一段，[`begin`](AudioOutput::begin) 会先停掉上一段。
#[derive(Default)]
pub struct AudioOutput {
    /// 每次 `begin()` / `stop()` 递增，用于判断某次播放是否已被后来的朗读抢占
    generation: AtomicU64,
    current: Mutex<Option<Active>>,
}

impl AudioOutput {
    /// 开一次新播放：抢占上一段 → 打开输出设备 → 返回本次会话句柄。
    pub fn begin(&self, sample_rate: u32, channels: u16) -> anyhow::Result<Playback> {
        let sample_rate = NonZero::new(sample_rate).context("采样率不能为 0")?;
        let channels = NonZero::new(channels).context("声道数不能为 0")?;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;

        // 先停上一段再开设备：两段声音叠在一起比没声音更糟
        self.clear_active();
        let (player, device) = open_device()?;

        *lock(&self.current) = Some(Active {
            generation,
            player: player.clone(),
            _device: device,
        });
        info!(
            "[Audio] 输出设备已打开 (session={}, {}Hz/{}ch)",
            generation, sample_rate, channels
        );
        Ok(Playback {
            generation,
            player,
            sample_rate,
            channels,
            leftover: Mutex::new(Vec::new()),
        })
    }

    /// 该次播放是否仍是当前这段（被后来的朗读抢占后返回 `false`）。
    pub fn is_current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    /// 停止当前播放并关闭设备（对应前端的「停止朗读」）。
    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.clear_active();
    }

    /// 播完收场：只在这一段仍是当前段时关设备，避免把后来者的设备关掉。
    /// 不递增 generation——此时并没有抢占谁。
    pub fn finish(&self, generation: u64) {
        let finished = {
            let mut current = lock(&self.current);
            match current.as_ref() {
                Some(active) if active.generation == generation => current.take(),
                _ => None,
            }
        };
        if let Some(active) = finished {
            active.player.stop();
            info!("[Audio] 输出设备已关闭 (session={})", generation);
        }
    }

    fn clear_active(&self) {
        let previous = lock(&self.current).take();
        if let Some(active) = previous {
            // 先让队列里的声音停掉，再 drop 设备句柄关闭设备
            active.player.stop();
            info!(
                "[Audio] 抢占并停止上一段播放 (session={})",
                active.generation
            );
        }
    }
}

/// 一次播放会话：把 PCM 分块 / 整段音频喂给输出设备。
///
/// 分块之间没有间隙——rodio 的播放队列在 source 交界处做了帧对齐与元数据前瞻，
/// 参数一致（这里恒为 24kHz 单声道）的相邻分块不会触发重采样器重置。
/// 队列临时见底时它自动补一小段静音而不是结束播放，正好吸收网络抖动。
pub struct Playback {
    generation: u64,
    player: Arc<Player>,
    sample_rate: NonZero<u32>,
    channels: NonZero<u16>,
    /// 跨分块残留的奇数尾字节（PCM16 必须按 2 字节对齐，否则整段变噪音）
    leftover: Mutex<Vec<u8>>,
}

impl Playback {
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 追加一块 PCM16LE 裸字节。
    pub fn push_pcm16(&self, chunk: &[u8]) {
        let samples = {
            let mut leftover = lock(&self.leftover);
            let bytes = if leftover.is_empty() {
                chunk.to_vec()
            } else {
                let mut merged = std::mem::take(&mut *leftover);
                merged.extend_from_slice(chunk);
                merged
            };
            let aligned = bytes.len() - bytes.len() % 2;
            leftover.clear();
            leftover.extend_from_slice(&bytes[aligned..]);
            pcm16_to_samples(&bytes[..aligned])
        };
        if samples.is_empty() {
            return;
        }
        self.player
            .append(SamplesBuffer::new(self.channels, self.sample_rate, samples));
    }

    /// 播放一段完整音频（mp3 / wav / flac / ogg，按内容嗅探）。
    pub fn play_encoded(&self, audio: Vec<u8>) -> anyhow::Result<()> {
        let decoder = Decoder::new(Cursor::new(audio))
            .context("音频解码失败：支持 mp3 / wav / flac / ogg-vorbis，不支持 opus 与裸 pcm")?;
        self.player.append(decoder);
        Ok(())
    }

    /// 丢弃已排入队列的音频（流刚开头就断、要整段重来时用）。
    ///
    /// 之后再 `push_pcm16` / `play_encoded` 会自动恢复播放（rodio 在 `append` 时清掉停止标记）。
    pub fn discard(&self) {
        self.player.stop();
        lock(&self.leftover).clear();
    }

    /// 队列是否已经播空。**只在所有音频都排完之后轮询**——流式过程中队列可能临时见底。
    pub fn is_drained(&self) -> bool {
        self.player.empty()
    }

    /// 当前 source 的播放位置。
    ///
    /// 用来判断输出设备是不是还在拉样本：设备中途掉了（拔耳机、被别的进程独占）时队列
    /// 永远播不空，只等 [`is_drained`](Self::is_drained) 会把这次朗读挂死。
    /// 队列里每个分块都是独立 source，位置会在分块交界处归零——**只看它变没变，别看它增没增**。
    pub fn position(&self) -> Duration {
        self.player.get_pos()
    }
}

/// PCM16LE 裸字节 → rodio 样本（`[-1.0, 1.0)`）。输入长度必须是 2 的倍数。
fn pcm16_to_samples(bytes: &[u8]) -> Vec<Sample> {
    bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as Sample / 32768.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm16_decodes_little_endian_samples() {
        // 0x0000 = 静音, 0x7FFF = 最大正, 0x8000 = 最小负
        let bytes = [0x00, 0x00, 0xFF, 0x7F, 0x00, 0x80];
        let samples = pcm16_to_samples(&bytes);
        assert_eq!(samples.len(), 3);
        assert!(samples[0].abs() < 1e-6);
        assert!((samples[1] - 1.0).abs() < 1e-3);
        assert!((samples[2] + 1.0).abs() < 1e-6);
    }

    // ── 真机用例 ────────────────────────────────────────────────────────
    // 需要一台真实输出设备，CI 上没有，所以默认 ignore：
    //   cargo test --lib -- --ignored --test-threads=1
    // 灌的是静音，不会真的发出声音；验的是「设备确实在按实时速度拉样本」——
    // 这正是 WebView 里失守的那条性质（那边队列照排、时钟照走，样本却没送到设备）。

    /// 1 秒静音 PCM 应该恰好花 ≈1 秒播完。
    #[test]
    #[ignore = "需要真实音频输出设备"]
    fn pcm_is_pulled_by_a_real_device_in_real_time() {
        let output = AudioOutput::default();
        let playback = output.begin(24_000, 1).expect("应能打开默认输出设备");

        // 分成 10 块推，顺带覆盖流式分块的排布路径
        for _ in 0..10 {
            playback.push_pcm16(&vec![0u8; 24_000 * 2 / 10]);
        }

        let started = std::time::Instant::now();
        while !playback.is_drained() && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let elapsed = started.elapsed();
        output.finish(playback.generation());

        assert!(playback.is_drained(), "1 秒音频 5 秒内没播完: {elapsed:?}");
        assert!(
            elapsed >= Duration::from_millis(700),
            "播得太快，说明样本没真的按实时速度送出去: {elapsed:?}"
        );
    }

    /// 整段音频（缓存重播走的就是这条）也要能解码并按实时速度播完。
    #[test]
    #[ignore = "需要真实音频输出设备"]
    fn encoded_audio_is_pulled_by_a_real_device_in_real_time() {
        // 1 秒 24kHz 单声道静音 WAV（与 tts::wrap_pcm16_wav 同格式）
        let pcm = vec![0u8; 24_000 * 2];
        let mut wav = Vec::with_capacity(44 + pcm.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&((36 + pcm.len()) as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&24_000u32.to_le_bytes());
        wav.extend_from_slice(&48_000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(&pcm);

        let output = AudioOutput::default();
        let playback = output.begin(24_000, 1).expect("应能打开默认输出设备");
        playback.play_encoded(wav).expect("WAV 应能解码");

        let started = std::time::Instant::now();
        while !playback.is_drained() && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let elapsed = started.elapsed();
        output.finish(playback.generation());

        assert!(playback.is_drained(), "1 秒音频 5 秒内没播完: {elapsed:?}");
        assert!(
            elapsed >= Duration::from_millis(700),
            "播得太快，说明样本没真的按实时速度送出去: {elapsed:?}"
        );
    }

    /// 抢占：新的一段开始时，上一段必须立刻停掉（否则两段声音叠在一起）。
    #[test]
    #[ignore = "需要真实音频输出设备"]
    fn begin_preempts_the_previous_playback() {
        let output = AudioOutput::default();
        let first = output.begin(24_000, 1).expect("应能打开默认输出设备");
        first.push_pcm16(&vec![0u8; 24_000 * 2 * 5]); // 5 秒
        let second = output.begin(24_000, 1).expect("应能再次打开输出设备");

        assert!(!output.is_current(first.generation()), "旧会话应已被抢占");
        assert!(output.is_current(second.generation()));
        output.finish(second.generation());
    }

    /// 上游分块长度不保证是偶数：错开一个字节整段就会变成噪音，
    /// 所以奇数尾字节必须留到下一块拼回去。
    #[test]
    fn odd_tail_byte_is_carried_to_next_chunk() {
        let leftover = Mutex::new(Vec::new());
        let align = |chunk: &[u8]| {
            let mut guard = lock(&leftover);
            let mut bytes = std::mem::take(&mut *guard);
            bytes.extend_from_slice(chunk);
            let aligned = bytes.len() - bytes.len() % 2;
            guard.extend_from_slice(&bytes[aligned..]);
            pcm16_to_samples(&bytes[..aligned])
        };

        assert_eq!(align(&[0x00, 0x00, 0x11]).len(), 1);
        assert_eq!(lock(&leftover).as_slice(), &[0x11]);
        // 上一块的 0x11 与这块的 0x22 拼成一个完整样本
        assert_eq!(align(&[0x22, 0x33, 0x44]).len(), 2);
        assert!(lock(&leftover).is_empty());
    }
}
