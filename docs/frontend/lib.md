# 工具库（lib/）

## 概述

前端工具函数层，封装 Tauri invoke 调用和语言数据定义。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src/lib/invoke.ts` | 类型化的 Tauri invoke 命令封装 |
| `src/lib/languages.ts` | 支持的语言列表定义 |
| `src/lib/windowUtils.ts` | 子窗口管理工具（吸附式窗口创建/focus） |
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
| `stopSpeaking()` | 停止当前朗读并熄灭高亮 |
| `isStreamPlaybackSupported()` | 是否支持 Web Audio（边收边播依赖） |

**关键机制：**

- **单例播放 + generation 抢占**：全局 `playGen` 计数，每次 `speak`/`speakSequence`/`stopSpeaking` 递增并停掉当前播放；全程用 `gen === playGen` 判断是否被后来的朗读打断，避免并发播放叠音
- **朗读状态**：写入 `stores/ttsStore.ts` 的 `speakingId`，对应按钮显示「停止」图标
- **前端 LRU 缓存**：`base_url\nmodel\nextra\ntext` 为键缓存完整音频（32 条），命中直接整段播；**流式分块播放不写前端缓存**（返回值不含完整音频），重播时靠后端缓存返回整段
- **AudioContext 单例**：全模块复用一个 `AudioContext`（`getAudioContext()`），播放结束只停 source 不 `close()`——WebKit 对同时存在的 context 数量有硬上限，每次朗读都 new+close 在连续朗读时容易踩到
- **流式边收边播**（`speech.stream_playback !== false` 且支持 Web Audio）：
  1. `new Channel<TtsStreamPayload>()` 作为 invoke 参数传给后端，`onmessage` 分派：`ArrayBuffer` → `pushChunk`，`{event:"start"}` → `setFormat`，`{event:"end"}` → `markInputComplete()`
  2. `StreamingPcmPlayer` 逐块 Int16→Float32（**不再经 base64**），`AudioContext.createBuffer(1, n, sampleRate)` 建块并按 `nextTime` 无缝排布（交给 ctx 重采样避免变调）；处理跨块奇数尾字节对齐
  3. 首块延后 `STREAM_PREROLL_SECONDS`(0.2s) 起播，吸收网络抖动，避免后续块稍慢就出现断续
  4. `ctx.state === "suspended"` 时 `resume()`：无用户手势的自动朗读下 context 会被挂起，不 resume 就一声不响什么都不播
  5. 收到 `end` 后所有已排块播完 → `done` 兑现；另有两层兜底防止「朗读中」不熄：invoke 返回 5s 后仍无 `end` 则按已收分块收尾，播放器内部 `armDrainWatchdog()` 在时间线走完后仍有未结束 source 时强制收口
  6. `chunkCount===0`（命中后端缓存/非流式协议/服务端不支持）→ 退回 `playWholeAudio` 整段播
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
- 流式播放假设分块为单声道 PCM16LE（后端 `start` 控制消息的 `sampleRate`/`channels` 决定重采样率）；后端改音频参数需同步此处
- **大块数据别走全局事件**：`app.emit` 会把负载拼进 `eval` 字符串广播给所有 webview，音频/图像这类高频大负载必须用 `Channel` + `InvokeResponseBody::Raw`
