# 本地音频输出（audio/）

## 概述

朗读的**播放**环节：把 PCM 分块 / 整段音频送进操作系统的输出设备（rodio + cpal）。
合成仍在 [tts/](tts.md)，这里只负责出声。

## 为什么播放在 Rust 侧而不是 WebView

主窗口失焦会自动隐藏（`App.tsx` 的 blur → `hide()`）。WKWebView 一旦被标记为遮挡，WebKit 就停掉
`AudioContext` 背后的音频单元，却仍用定时器时钟继续「空转渲染」：

- `ctx.state` 是 `running`、`currentTime` 正常推进
- 分块照常排进时间线、`onended` 照常触发
- **样本根本没送到输出设备**，而 JS 侧查不到任何异常状态位

快捷键翻译的典型流程恰恰是「窗口弹出 → 焦点回到原 App → 窗口隐藏」，朗读几乎每次都发生在
窗口隐藏期间，于是表现为「日志一切正常（分块全部排入、耗时≈音频时长、峰值正常）但一声不响」。

前端先后试过**预热输出设备**、**每次朗读重建 `AudioContext`**、**比对墙上时钟与音频时长做哑火兜底**
（`1c1df3d` / `ce78186` / `4844b16`），都挡不住——重建只能挡「朗读**前**就隐藏」，挡不住
「朗读**中**隐藏」；而哑火兜底的判据是「墙上时钟耗时远小于音频时长」，空转渲染恰恰是按实时速度走的，
判定为正常。

挪到 Rust 侧后：

| | WebView（Web Audio） | Rust（rodio/cpal） |
|---|---|---|
| 窗口隐藏 | 音频单元被停，静音 | 无关，照常出声 |
| 设备冷启动 | 按时间线排程，排在启动期间的音频**被吞掉** | 样本被设备**按需拉取**，晚启动只是晚开始 |
| PCM 传输 | 每块走一次 IPC（几百块 / 长文本） | 不过 IPC |
| 哑火可观测性 | JS 侧无状态位 | 播放位置不推进即可判定（见「播放停滞」） |

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/audio/mod.rs` | 输出设备的开关、抢占、PCM/整段音频入队、播放进度 |

## 核心逻辑

### `AudioOutput` — 进程级音频输出（挂在 `AppState.audio`）

同一时刻只播一段。内部是 `generation: AtomicU64` + `current: Mutex<Option<Active>>`。

| 方法 | 说明 |
|------|------|
| `begin(sample_rate, channels) -> Result<Playback>` | 递增 generation → **停掉上一段** → 打开输出设备 → 返回本次会话句柄 |
| `is_current(generation) -> bool` | 该次播放是否仍是当前段（被后来的朗读抢占后为 `false`） |
| `stop()` | 停止当前播放并关闭设备（对应前端「停止朗读」） |
| `finish(generation)` | 播完收场：**仅当这一段仍是当前段**时关设备，不递增 generation |

> `finish` 必须比对 generation：命令层「等播完」的循环退出后才调它，这期间可能已经被新的朗读
> 抢占，无条件关设备会把后来者的设备关掉。

### `Playback` — 一次播放会话

| 方法 | 说明 |
|------|------|
| `push_pcm16(&[u8])` | 追加一块 PCM16LE 裸字节（流式路径） |
| `play_encoded(Vec<u8>)` | 播放整段音频，按内容嗅探格式（缓存命中 / audio\_speech 协议 / 非流式） |
| `discard()` | 丢弃已排入队列的音频（流刚开头就断、要整段重来时用） |
| `is_drained()` | 队列是否播空。**只在所有音频都排完之后轮询**——流式过程中队列可能临时见底 |
| `position()` | 当前 source 的播放位置，用于判断设备是否还在拉样本 |

**分块对齐**：`push_pcm16` 维护 `leftover`，把奇数尾字节留到下一块拼回去。上游分块长度不保证是偶数，
错开一个字节整段就会变成噪音。

**分块之间没有间隙**：rodio 的播放队列在 source 交界处做帧对齐、并向 `UniformSourceIterator`
前瞻下一个 source 的元数据，参数一致（这里恒为 24kHz 单声道）的相邻分块不会触发重采样器重置。
队列临时见底时它自动补一小段静音而不是结束播放，正好吸收网络抖动。

### 设备生命周期

`MixerDeviceSink` 内含 `cpal::Stream`（`!Send`），只能待在创建它的线程上。于是：

1. `open_device()` 起一个名为 `dh-tts-audio` 的线程，在里面打开设备、建 `Player`
2. `Player`（`Send + Sync`）通过 channel 交回调用方，之后任意线程都能 `append` / `stop`
3. 那个线程随后阻塞在 `Condvar` 上守着设备；`DeviceHandle` 被 drop 时唤醒它退出，设备随之关闭

**每次朗读都重新开设备**，而不是全程复用一个：cpal 的输出流绑定的是**打开时**的那个设备，用户中途
连上蓝牙耳机 / 拔掉外接音箱时不会自动跟随，复用就会播到已经不用的设备上去。开设备的开销被合成请求的
1s+ 网络往返吸收，听感上不增加起播延迟。

### 锁中毒

`lock()` 助手在 `PoisonError` 时照常取出数据。音频状态全是「当前播的是哪一段」这种可重建的信息，
为它把整个朗读功能 panic 掉不值当。

## 音频格式支持

`play_encoded` 走 rodio 的 `Decoder`（symphonia）。Cargo 特性开的是 `playback` + **伞形**特性
`wav` / `mp3` / `flac` / `vorbis`：

- ⚠️ 只开底层的 `symphonia-wav`（RIFF 容器）而不开 `symphonia-pcm`（PCM 编解码），解码会直接
  `UnrecognizedFormat`——**缓存里的 WAV 重播不出来**。`wav` 这个伞形特性两个都带上。
  `tts::tests::wrapped_wav_is_decodable_by_the_player` 锁死这条
- **不支持 opus 与裸 pcm**：symphonia 没有 opus 解码器，裸 pcm 没有容器。`audio/speech` 的
  `response_format` 只应填 `mp3` 或 `wav`（设置界面的 tooltip 已注明）
- 默认特性里的 `recording` **刻意不开**：那会把麦克风采集一起编进来

## 播放停滞

输出设备中途掉了（拔耳机、被别的进程独占）时队列永远播不空，只等 `is_drained()` 会把命令挂死、
前端加载态一直转。命令层因此同时盯 `position()`：**位置 10s 没变**就判定设备不在拉样本，提前收场。

> 队列里每个分块都是独立 source，位置会在分块交界处归零——**只看它变没变，别看它增没增**。

## 依赖关系

- **依赖**：`rodio`（含 `cpal`、`symphonia`）、`anyhow`、`log`
- **被依赖**：`config::AppState.audio`、`commands::tts::{speak_text, stop_speech}`

## 修改指南

- 改采样率/声道要同步 `tts::stream_sample_rate()` / `stream_channels()`（`begin()` 的入参来自那里）
- 新增音频格式时改 `Cargo.toml` 的 rodio 特性，**用伞形特性**（`wav`/`mp3`/…），别只开 `symphonia-*` 容器
- 抢占语义（generation）改动时同步命令层的「等播完」循环：那里靠 `is_current()` 退出
- 真机用例默认 `#[ignore]`（CI 无音频设备），本地验证跑
  `cargo test --lib -- --ignored --test-threads=1`。灌的是静音，不会发出声音；验的是
  **设备确实在按实时速度拉样本**——正是 WebView 里失守的那条性质
- 日志前缀：`[Audio]`
