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

Channel 只投递给发起调用的 webview，大负载走 IPC 自定义协议（fetch）而非 `eval` 字符串；全局事件（`app.emit`）会把每个分块的 base64 拼进 eval 脚本广播给**所有** webview，长文本几百个分块时直接堵死主线程（表现为「必须等流传完才播 / 长文本播不出来」）。通道消息保证按发送顺序投递，因此 `end` 一定排在所有分块之后。

### tts.ts - 共享朗读模块

统一的朗读入口，被 `ActionButtons`（手动朗读）和 `useTranslation`（翻译后自动朗读）共用。

**导出函数：**

| 函数 | 说明 |
|------|------|
| `speak(text, id)` | 朗读一段文本；`id`（如 `"source"`/`"target"`）用于「朗读中」高亮。会抢占正在进行的朗读，播完/出错/被打断时兑现 |
| `speakSequence(items)` | 依次朗读多段（`[{text,id}]`）；前一段播完再播下一段，被打断则整体中止（自动朗读原文→译文用） |
| `stopSpeaking()` | 停止当前朗读并熄灭高亮/加载态 |
| `primeAudio()` | 预热音频链路：提前建好 `AudioContext`、resume、排一段静音唤醒输出设备。由 `App.tsx` 在挂载时与主窗口获得焦点时调用 |
| `isStreamPlaybackSupported()` | 是否支持 Web Audio（边收边播依赖） |
| `countSpeechUnits(text)` | 统计文本长度：CJK（含假名/谚文）按字计 + 其余语种按 `[\p{L}\p{N}]+` 单词计，两者相加。供「自动朗读长度上限」（`speech.auto_read_max_units`）判断 |

**关键机制：**

- **单例播放 + generation 抢占**：全局 `playGen` 计数，每次 `speak`/`speakSequence`/`stopSpeaking` 递增并停掉当前播放；全程用 `gen === playGen` 判断是否被后来的朗读打断，避免并发播放叠音
- **朗读状态**：写入 `stores/ttsStore.ts` 的 `speakingId`，对应按钮显示「停止」图标
- **加载状态**：`ttsStore.loadingId` 标记「已发起合成、音频还没到」的窗口期（按钮转圈）。发起请求前置位，**收到第一段音频数据时熄灭**——流式路径由 `StreamingPcmPlayer` 的 `onFirstAudio`（首块 PCM 排入播放）回调，整段路径由 `playWholeAudio` 的 `onStart`（`audio.play()` 兑现）回调；`playOne` 的 `finally` 兜底清除。命中前端缓存时不进入加载态。所有清除都带 `gen === playGen` 守卫，避免被抢占的旧会话熄掉新会话的加载态
- **前端 LRU 缓存**：`base_url\nmodel\nextra\ntext` 为键缓存完整音频（32 条），键里的三项取自 `resolveActiveProvider()`（与后端 `ServiceConfig::resolved` 同规则，含 provider 级 extra 覆盖）；命中直接整段播；**流式分块播放不写前端缓存**（返回值不含完整音频），重播时靠后端缓存返回整段
- **AudioContext 生命周期**：同一时刻只存在一个 context（`getAudioContext()`，WebKit 对并存数量有硬上限），但**不跨朗读会话复用**——`playOne` 每次流式朗读前调 `ensureAudioContextRunning(true)`，由 `resetAudioContext()` 先 `close()` 旧的再建新的
  - 原因：主窗口失焦会自动隐藏，窗口一隐藏 WKWebView 即被标记为遮挡、底层音频单元停止；窗口再显示时 WebKit **不会**为仍处于 `running` 的 context 重新拉起音频单元，而是继续用定时器驱动的时钟「空转渲染」——`state` 仍是 `running`、`currentTime` 正常推进、`onended` 照常触发、`playedThrough` 判定为正常，样本却没送到输出设备。表现为「**第一次朗读有声，之后每次完全无声，手动点一下走 `<audio>` 整段播又正常**」
  - 这种哑火在 JS 侧没有任何可查的状态位（所以哑火兜底抓不到），只能靠换新 context 规避；重建成本被合成请求的 1s+ 往返与静音预热吸收，不增加起播延迟
  - 副作用：每次流式朗读都会打一条 `[TTS] AudioContext 已创建, state=..., sampleRate=...`，可据此确认重建生效
- **音频链路预热**（`primeAudio()` / `ensureAudioContextRunning()` / `warmUpOutputDevice()`）：
  - `AudioContext` 刚创建或刚 `resume()` 时底层输出设备还在启动（蓝牙耳机可达 1s 以上），这期间已排上时间线的 buffer 会被**直接吞掉**——旧实现把 context 懒创建在第一块 PCM 到达时，于是「第一次朗读（尤其是翻译后自动朗读）没声音，再点一次就正常」；整段播放（`<audio>`）不受影响是因为媒体元素自己会等设备就绪，所以症状看起来像「流式播放在自动播放时失效」
  - 修复三件套：① `App.tsx` 挂载时和主窗口获得焦点时 `primeAudio()` 提前建好 context；② `playOne` 在**发合成请求前** `await ensureAudioContextRunning(true)`（重建 + resume 并等待完成 + 排一段 `AUDIO_WARMUP_SECONDS`(0.35s) 静音唤醒设备），合成的 1s+ 往返正好当预热时间；③ 首块起播时刻取 `max(currentTime + 预留, warmupUntil)`，保证真实音频一定排在预热之后
  - `ensureAudioContextRunning()` 返回 `null`（Web Audio 不可用 / resume 后仍非 `running`，如平台要求用户手势）时，本次朗读**回退整段播放**而不是静默失败；`primeAudio()` 另挂一次性 `pointerdown`/`keydown` 监听兜底解锁
- **流式边收边播**（`speech.stream_playback !== false` 且 context 处于 `running`）：
  1. `new Channel<TtsStreamPayload>()` 作为 invoke 参数传给后端，`onmessage` 分派：`ArrayBuffer` → `pushChunk`，`{event:"start"}` → `setFormat`，`{event:"end"}` → `markInputComplete()`
  2. `StreamingPcmPlayer` 逐块 Int16→Float32（**不再经 base64**），`AudioContext.createBuffer(1, n, sampleRate)` 建块并按 `nextTime` 无缝排布（交给 ctx 重采样避免变调）；处理跨块奇数尾字节对齐
  3. 首块起播时刻 = `max(currentTime + STREAM_PREROLL_SECONDS(0.2s), warmupUntil)`：0.2s 预留吸收网络抖动、避免后续块稍慢就断续，`warmupUntil` 保证不会抢在输出设备预热完成之前
  4. 播放途中 `ctx.state === "suspended"`（系统挂起 / 设备切换）时补一次 `resume()`；起播前的 resume 由 `ensureAudioContextRunning()` 负责
  5. 收到 `end` 后所有已排块播完 → `done` 兑现；另有两层兜底防止「朗读中」不熄：invoke 返回 5s 后仍无 `end` 则按已收分块收尾，播放器内部 `armDrainWatchdog()` 在时间线走完后仍有未结束 source 时强制收口
  6. `chunkCount===0`（命中后端缓存/非流式协议/服务端不支持）→ 退回 `playWholeAudio` 整段播
  7. **哑火兜底**（`player.playedThrough`）：`done` 兑现后比对「墙上时钟耗时」与「音频总时长」，前者不足后者一半 → 判定分块虽排进了时间线却没真正出声，再调一次 `synthesize_speech`（**后端缓存必然命中，不会重新合成**）拿完整音频走 `playWholeAudio`。注意这一层**只能抓住「时间线根本没走」的哑火**；context「空转渲染」那种（时间线照常走、只是没送到设备）它判定为正常，靠的是每次重建 context 来预防
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
- 流式播放假设分块为单声道 PCM16LE（后端 `start` 控制消息的 `sampleRate`/`channels` 决定重采样率）；后端改音频参数需同步此处
- **大块数据别走全局事件**：`app.emit` 会把负载拼进 `eval` 字符串广播给所有 webview，音频/图像这类高频大负载必须用 `Channel` + `InvokeResponseBody::Raw`
