# 工具库（lib/）

## 概述

前端工具函数层，封装 Tauri invoke 调用和语言数据定义。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src/lib/invoke.ts` | 类型化的 Tauri invoke 命令封装 |
| `src/lib/languages.ts` | 支持的语言列表定义 |
| `src/lib/windowUtils.ts` | 子窗口管理工具（吸附式 / 居中独立窗口的创建与 focus） |
| `src/lib/tts.ts` | 共享朗读模块：前端缓存、流式边收边播、单例播放/顺序朗读 |

## 核心逻辑

### invoke.ts - Tauri 命令封装

对 `@tauri-apps/api/core` 的 `invoke()` 进行类型化封装，确保前后端接口类型安全。

| 函数 | 参数 | 返回值 | 对应后端命令 |
|------|------|--------|-------------|
| `startRegionSelect(mode)` | `mode: string` | `Promise<void>` | `start_region_select` |
| `captureRegion(monitorIndex, x, y, width, height)` | 5 个 number | `Promise<string>` | `capture_region` |
| `getFrozenScreenshot(monitorIndex)` | `monitorIndex: number` | `Promise<ScreenshotInitEvent>` | `get_frozen_screenshot` |
| `captureAndOcr(monitorIndex, x, y, width, height, language)` | 5 个 number + string | `Promise<string>` | `capture_and_ocr` |
| `translateText(text, sourceLang, targetLang)` | 3 个 string | `Promise<string>` | `translate_text` |
| `getSettings()` | — | `Promise<Settings>` | `get_settings` |
| `saveSettings(settings)` | `settings: Settings` | `Promise<void>` | `save_settings` |
| `readClipboard()` | — | `Promise<string>` | `read_clipboard` |
| `readSelectedText()` | — | `Promise<string>` | `read_selected_text` |
| `copyImageToClipboard(imageBase64)` | `imageBase64: string` | `Promise<void>` | `copy_image_to_clipboard` |
| `synthesizeSpeech(text)` | `text: string` | `Promise<string>` | `synthesize_speech` |
| `synthesizeSpeechStream(text, onChunk)` | `text: string, onChunk: Channel<TtsStreamPayload>` | `Promise<SpeechResponse>` | `synthesize_speech_stream` |

**注意：** Tauri invoke 的参数名使用 camelCase，Tauri 会自动转换为后端的 snake_case。

`SpeechResponse = { audio: string; chunkCount: number; sampleRate: number }`：`chunkCount > 0` 表示走了流式分块（音频已由通道逐块送达，此时 `audio` 为空串，避免长文本重复传输数 MB）；`0` 表示应直接播放 `audio`（完整音频 base64，同时写前端缓存）。

流式分块走 **IPC Channel** 而非全局事件：

```typescript
type TtsStreamMessage =
  | { event: "start"; sampleRate: number; channels: number }
  | { event: "end"; chunkCount: number };
type TtsStreamPayload = ArrayBuffer | TtsStreamMessage;  // 二进制 = PCM16LE 分块
```

Channel 只投递给发起调用的 webview，**大负载的数据体**走 IPC 自定义协议（fetch）而非 `eval` 字符串；全局事件（`app.emit`）会把每个分块的 base64 拼进 eval 脚本广播给**所有** webview，长文本几百个分块时直接堵死主线程（表现为「必须等流传完才播 / 长文本播不出来」）。通道消息保证按发送顺序投递，因此 `end` 一定排在所有分块之后。

> 注意 Channel 的每条消息**仍会 eval 一小段触发代码**（Tauri 2.11.5 `ipc/channel.rs`），走 fetch 的只是负载本身：`Raw` <1KB 内联成 `new Uint8Array([...]).buffer`，≥1KB 存进 `ChannelDataIpcQueue` 由前端 fetch 取回。两条路径前端都拿到 `ArrayBuffer`；只有自定义协议 IPC 整体失败回退 `postMessage` 时形态会变，所以分块分派前统一过一道 `asArrayBuffer()`（兼容 TypedArray / number[]），否则会被误当控制消息**静默丢掉**，症状是「完全没声音且日志毫无线索」。

### tts.ts - 共享朗读模块

统一的朗读入口，被 `ActionButtons`（手动朗读）和 `useTranslation`（翻译后自动朗读）共用。

**导出函数：**

| 函数 | 说明 |
|------|------|
| `speak(text, id)` | 朗读一段文本；`id`（如 `"source"`/`"target"`）用于「朗读中」高亮。会抢占正在进行的朗读，播完/被打断时兑现，**合成失败会 reject**（调用方必须 `.catch`，否则是 unhandled rejection） |
| `speakSequence(items)` | 依次朗读多段（`[{text,id}]`）；前一段播完再播下一段，被打断则整体中止（自动朗读原文→译文用）。**单段失败只打日志、不牵连后续段**——原文合成挂了译文该读还是要读 |
| `stopSpeaking()` | 停止当前朗读并熄灭高亮/加载态 |
| `primeAudio()` | 预热音频链路：提前建好 `AudioContext`、resume、排一段循环静音把输出设备拽着跑。由 `App.tsx` 在挂载时与主窗口获得焦点时调用 |
| `isStreamPlaybackSupported()` | 是否支持 Web Audio（边收边播依赖） |
| `countSpeechUnits(text)` | 统计文本长度：CJK（含假名/谚文）按字计 + 其余语种按 `[\p{L}\p{N}]+` 单词计，两者相加。供「自动朗读长度上限」（`speech.auto_read_max_units`）判断 |

**关键机制：**

- **单例播放 + generation 抢占**：全局 `playGen` 计数，每次 `speak`/`speakSequence`/`stopSpeaking` 递增并停掉当前播放；全程用 `gen === playGen` 判断是否被后来的朗读打断，避免并发播放叠音
- **朗读状态**：写入 `stores/ttsStore.ts` 的 `speakingId`，对应按钮显示「停止」图标
- **加载状态**：`ttsStore.loadingId` 标记「已发起合成、音频还没到」的窗口期（按钮转圈）。发起请求前置位，**收到第一段音频数据时熄灭**——流式路径由 `StreamingPcmPlayer` 的 `onFirstAudio`（首块 PCM 排入播放）回调，整段路径由 `playWholeAudio` 的 `onStart`（`audio.play()` 兑现）回调；`playOne` 的 `finally` 兜底清除。命中前端缓存时不进入加载态。所有清除都带 `gen === playGen` 守卫，避免被抢占的旧会话熄掉新会话的加载态
- **前端 LRU 缓存**：`base_url\nmodel\nextra\ntext` 为键缓存完整音频，键里的三项取自 `resolveActiveProvider()`（与后端 `ServiceConfig::resolved` 同规则，含 provider 级 extra 覆盖）；命中直接整段播
  - **条数与总字符数双上限**（32 条 / 32M 字符）：只限条数不够，一分钟语音的 WAV base64 就有 ~3.8MB，塞满能吃掉上百 MB JS 堆。覆盖同键时回退计数，单条自身超预算时保留不自我淘汰
  - **流式路径播完会本地回填**：后端流式返回值不含完整音频（`audio` 被清空），播放器把已收到的 PCM 原样留一份，播完（且收到 `end`）用 `wrapPcm16Wav` + `bytesToBase64` 在本地拼出完整 WAV 写进缓存。不回填的话重播只能再走一趟 IPC 把几 MB base64 从后端搬回来（还会白建一次 AudioContext）
  - 本地留存有 16MB 上限（`STREAM_PCM_KEEP_MAX_BYTES`，≈5.8 分钟音频）：超了就放弃留存（打 warn），此时不回填缓存、哑火兜底改为回后端要。AudioBuffer 本身已占 2 倍于 PCM 的堆，再叠 PCM + base64 三份对超长朗读太重
  - 前端拼出的 WAV 与后端 `wrap_pcm16_wav` **字节完全一致**（后端 `wav_bytes_match_frontend_implementation` 用固定向量锁死），两边的 WAV 才能在同一套缓存语义里互换
- **AudioContext 生命周期**：同一时刻只存在一个 context（`getAudioContext()`，WebKit 对并存数量有硬上限），但**不跨朗读会话复用**——`playOne` 每次流式朗读前调 `ensureAudioContextRunning(true)`，由 `resetAudioContext()` 先 `close()` 旧的再建新的
  - 原因：主窗口失焦会自动隐藏，窗口一隐藏 WKWebView 即被标记为遮挡、底层音频单元停止；窗口再显示时 WebKit **不会**为仍处于 `running` 的 context 重新拉起音频单元，而是继续用定时器驱动的时钟「空转渲染」——`state` 仍是 `running`、`currentTime` 正常推进、`onended` 照常触发、`playedThrough` 判定为正常，样本却没送到输出设备。表现为「**第一次朗读有声，之后每次完全无声，手动点一下走 `<audio>` 整段播又正常**」
  - 这种哑火在 JS 侧没有任何可查的状态位（所以哑火兜底抓不到），只能靠换新 context 规避；重建成本被合成请求的 1s+ 往返与静音预热吸收，不增加起播延迟
  - 副作用：每次流式朗读都会打一条 `[TTS] AudioContext 已创建, state=..., sampleRate=...`，可据此确认重建生效
- **音频链路预热**（`primeAudio()` / `ensureAudioContextRunning()` / `warmUpOutputDevice()`）：
  - `AudioContext` 刚创建或刚 `resume()` 时底层输出设备还在启动（蓝牙耳机可达 1s 以上），这期间已排上时间线的 buffer 会被**直接吞掉**——旧实现把 context 懒创建在第一块 PCM 到达时，于是「第一次朗读（尤其是翻译后自动朗读）没声音，再点一次就正常」；整段播放（`<audio>`）不受影响是因为媒体元素自己会等设备就绪，所以症状看起来像「流式播放在自动播放时失效」
  - 修复三件套：① `App.tsx` 挂载时和主窗口获得焦点时 `primeAudio()` 提前建好 context；② `playOne` 在**发合成请求前** `await ensureAudioContextRunning(true)`（重建 + resume 并等待完成 + `warmUpOutputDevice()`），合成的 1s+ 往返正好当预热时间；③ 首块起播时刻取 `max(currentTime + 预留, warmupUntil)`，保证真实音频一定排在预热之后
  - `warmUpOutputDevice()` 排的是**循环静音**（`loop = true` + `stop(currentTime + AUDIO_KEEPALIVE_SECONDS)`，30s 自动收，不用定时器），不是一段固定长度的静音。早先用 0.35s 一次性静音，两个问题：设备启动比 0.35s 慢时照样吞开头；静音放完到首块 PCM 到达之间还有 1s+ 空档（合成 + 网络往返），设备可能又空转下去。`AUDIO_WARMUP_SECONDS`(0.35s) 现在只作为「首块至少比预热开始晚这么多」的下限
  - ⚠️ 这一条尚未在真机（有线 / 蓝牙两种输出）实测确认，只推理和编译验证过
  - `ensureAudioContextRunning()` 返回 `null`（Web Audio 不可用 / resume 后仍非 `running`，如平台要求用户手势）时，本次朗读**回退整段播放**而不是静默失败；`primeAudio()` 另挂一次性 `pointerdown`/`keydown` 监听兜底解锁
- **流式边收边播**（`speech.stream_playback !== false` 且 context 处于 `running`）：
  1. `new Channel<TtsStreamPayload>()` 作为 invoke 参数传给后端，`onmessage` 分派：先过 `asArrayBuffer()`，是二进制 → `pushChunk`；否则按 `{event:"start"}` → `setFormat` / `{event:"end"}` → `markInputComplete()`，**两者都不是就打 warn**（静默丢弃等于「没声音且无线索」）
  2. `StreamingPcmPlayer` 逐块 Int16→Float32（**不再经 base64**），`AudioContext.createBuffer(1, n, sampleRate)` 建块并按 `nextTime` 无缝排布（交给 ctx 重采样避免变调）；处理跨块奇数尾字节对齐；同时把对齐后的 PCM 原样留一份用于播完拼 WAV 回填缓存
  3. 首块起播时刻 = `max(currentTime + STREAM_PREROLL_SECONDS(0.2s), warmupUntil)`：0.2s 预留吸收网络抖动、避免后续块稍慢就断续，`warmupUntil` 保证不会抢在输出设备预热完成之前
  4. 播放途中 `ctx.state === "suspended"`（系统挂起 / 设备切换）时补一次 `resume()`；起播前的 resume 由 `ensureAudioContextRunning()` 负责
  5. 收到 `end` 后所有已排块播完 → `done` 兑现；另有两层兜底防止「朗读中」不熄：
     - `setInterval` 每秒检查，**分块已停止到达超过 `STREAM_IDLE_TIMEOUT_MS`(3s) 且仍无 `end`** 才按已收分块收尾。判据必须是「静默时长」而不是「invoke 返回后固定 N 秒」——命令返回时分块往往还在路上，定时收口会在播放中途提前兑现 `done`，让哑火兜底和还在播的流式音频**叠在一起且停不掉**
     - 播放器内部 `armDrainWatchdog()` 在时间线走完后仍有未结束 source 时强制收口
  6. `chunkCount===0`（命中后端缓存/非流式协议/服务端不支持）→ 退回 `playWholeAudio` 整段播
  7. **合成 reject 时必须 `player.stop()` 再抛**：分块可能已经排进时间线，不停掉就成了「UI 已熄灭、`stopSpeaking()` 也抓不到」的幽灵音频（`stopCurrent` 会被 `speak()` 的 finally 清空）
  8. **哑火兜底**（`done` 兑现后）：
     - 判据①「后端 `chunkCount > 0` 而 `player.scheduledChunks === 0`」——分块全被丢弃，必然静音，最确定；顺带对 `chunkCount - scheduledChunks > 0` 打 warn
     - 判据②`player.playedThrough`——比对「墙上时钟耗时」与「音频总时长」，前者不足后者一半即判定排进了时间线却没真正出声。这层**只能抓住「时间线根本没走」的哑火**；context「空转渲染」那种（时间线照常走、只是没送到设备）它判定为正常，靠的是每次重建 context 来预防
     - 兜底前**先 `player.stop()`**：兜底走 `<audio>` 会把 `stopCurrent` 改写成 `audio.pause`，播放器还活着就会两路重叠且再也停不掉
     - 音频来源优先本地拼的完整 WAV（省一次几 MB 的 IPC 往返），拿不到才 `synthesize_speech`（后端缓存必然命中，不会重新合成）；并带上 `clearLoading`，因为一块都没排上时 `onFirstAudio` 从未触发，不然按钮转到整段播完
- **诊断日志**（排查上述哑火用，见 `StreamingPcmPlayer`）：
  - 首块：`ctx采样率` / `PCM采样率` / `块时长` / **`峰值`**（≈0 说明拿到的 PCM 本身就是静音，问题在后端而非播放）
  - 上游流结束：`已排入` / `待播` / `剩余时间线`
  - 收尾：**`实际耗时` vs `音频总时长`**（前者远小于后者 = 时间线没真走，设备哑火）、`已播完分块` / `被中止`
- **整段播放**（`playWholeAudio`）：`detectAudioMime` 嗅探魔数 → `new Audio("data:{mime};base64,...")`

### languages.ts - 语言列表

**Language 接口：**
```typescript
interface Language { code: string; name: string }
```

**支持的 15 种语言：**

| code | name |
|------|------|
| `auto` | Auto Detect |
| `zh-CN` | Chinese (Simplified) |
| `zh-TW` | Chinese (Traditional) |
| `en` | English |
| `ja` | Japanese |
| `ko` | Korean |
| `fr` | French |
| `de` | German |
| `es` | Spanish |
| `pt` | Portuguese |
| `ru` | Russian |
| `ar` | Arabic |
| `it` | Italian |
| `th` | Thai |
| `vi` | Vietnamese |

**导出：**
- `languages` — 包含 `auto` 的完整列表（用于源语言选择）
- `targetLanguages` — 过滤掉 `auto` 的列表（用于目标语言选择）

### windowUtils.ts - 子窗口创建

两种子窗口形态，都是「已存在则 focus，否则新建」，统一 `decorations:false + transparent:true + resizable:true`：

| 函数 | 形态 | 使用方 |
|------|------|--------|
| `openDockedWindow({label, url, title, width, side, gap?})` | 吸附在主窗口左/右侧，高度跟随主窗口 | 调试日志窗口（右侧 360px） |
| `openCenteredWindow({label, url, title, width, height, minWidth?, minHeight?})` | 屏幕居中的独立窗口 | 设置窗口（720×540，最小 640×440） |

设置窗口曾经也是吸附式（左侧 340px），但配置项太多，窄栏单列滚动导致信息过载，已改为居中独立窗口 + 左侧分区导航。

## 依赖关系

- **依赖**：`@tauri-apps/api/core`（invoke、Channel）、`@tauri-apps/api/window`（getCurrentWindow）、`@tauri-apps/api/webviewWindow`（WebviewWindow）、`types/index.ts`（Settings）
- **被依赖**：
  - `invoke.ts` → `hooks/useScreenshot`、`hooks/useTranslation`、`App.tsx`、`SettingsPanel`、`LogPanel`、`lib/tts.ts`
  - `languages.ts` → `components/translation/LanguageSelector.tsx`
  - `windowUtils.ts` → `stores/logStore.ts`（openDebugWindow）、`stores/settingsStore.ts`（openSettingsWindow）
  - `tts.ts` → `components/translation/ActionButtons.tsx`、`hooks/useTranslation.ts`；读 `stores/settingsStore`、`stores/ttsStore`

## 修改指南

- 新增 Tauri 命令时同步添加 invoke 封装函数，保持类型安全
- invoke 参数名必须与后端 `#[tauri::command]` 函数参数名的 camelCase 形式一致
- 新增语言需同时更新 `languages` 数组，并确认后端 OCR 模块支持该语言
- `auto` 语言仅适用于源语言，`targetLanguages` 会自动排除
- 朗读逻辑集中在 `tts.ts`：新增播放入口应复用 `speak`/`speakSequence` 以共享单例抢占与缓存，避免多处 `new Audio` 叠音
- **不要把 `AudioContext` 的创建/`resume` 推迟到音频数据到达时**：设备冷启动会吞掉开头的声音，必须经 `ensureAudioContextRunning()` 提前就绪，并让首块起播不早于 `warmupUntil`
- 新增播放路径时记得接上加载态回调（首帧数据到达即 `setLoadingId(null)`），否则按钮会一直转圈到播放结束
- **任何「换一路音频接着播」的分支，动手前先把上一路停掉**：`stopCurrent` 是单个引用，后写的一路会盖掉前一路的 stop 函数，前一路就此失控（重叠 + 停不掉）。哑火兜底那处就是这么踩过的
- **改 `wrapPcm16Wav` 必须同步后端 `wrap_pcm16_wav`**，两边字节要完全一致，否则同一段文本在前后端缓存里拿到的音频不同；后端 `wav_bytes_match_frontend_implementation` 会先报错
- 流式收口判据用「分块静默时长」而非「固定超时」：命令返回时分块常常还在路上，定时收口会让 `done` 提前兑现、把兜底和还在播的流式音频叠一起
- 流式播放假设分块为单声道 PCM16LE（后端 `start` 控制消息的 `sampleRate`/`channels` 决定重采样率）；后端改音频参数需同步此处
- **大块数据别走全局事件**：`app.emit` 会把负载拼进 `eval` 字符串广播给所有 webview，音频/图像这类高频大负载必须用 `Channel` + `InvokeResponseBody::Raw`
