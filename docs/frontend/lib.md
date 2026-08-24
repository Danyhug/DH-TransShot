# 工具库（lib/）

## 概述

前端工具函数层，封装 Tauri invoke 调用和语言数据定义。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src/lib/invoke.ts` | 类型化的 Tauri invoke 命令封装 |
| `src/lib/languages.ts` | 支持的语言列表定义 |
| `src/lib/windowUtils.ts` | 子窗口管理工具（吸附式 / 居中独立窗口的创建与 focus） |
| `src/lib/tts.ts` | 共享朗读模块：单例抢占、朗读中/加载中状态、长度统计（**播放本身在 Rust 侧**） |

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
| `speakText(text, onEvent)` | `text: string, onEvent: Channel<TtsPlaybackMessage>` | `Promise<void>`（**播完才 resolve**） | `speak_text` |
| `stopSpeech()` | — | `Promise<void>` | `stop_speech` |

**注意：** Tauri invoke 的参数名使用 camelCase，Tauri 会自动转换为后端的 snake_case。

朗读的**音频数据完全不过 IPC**：后端 `speak_text` 合成后直接把 PCM / 整段音频送进操作系统输出设备（见 [docs/backend/audio.md](../backend/audio.md)）。因此这条通道上只剩一种控制消息：

```typescript
export type TtsPlaybackMessage = { event: "start" };
```

含义是「第一段音频已送入输出设备」，前端据此熄灭按钮的加载态。`speakText()` 的 promise 在**播完之后**才兑现（被后来的朗读抢占时后端提前收场并正常返回），所以前端不需要额外的定时器或看门狗。

> 曾经这里是几百个 PCM 分块走 Channel 二进制、前端 `asArrayBuffer()` 兼容各种投递形态、再由 `StreamingPcmPlayer` 排进 `AudioContext`。整套连同 `AudioContext` 预热/重建/哑火兜底一起删掉了，原因见下。

### tts.ts - 共享朗读模块

统一的朗读入口，被 `ActionButtons`（手动朗读）和 `useTranslation`（翻译后自动朗读）共用。

**这里只做编排**——合成、缓存、播放全在 Rust 侧（[backend/tts.md](../backend/tts.md) + [backend/audio.md](../backend/audio.md)）。

**导出函数：**

| 函数 | 说明 |
|------|------|
| `speak(text, id)` | 朗读一段文本；`id`（如 `"source"`/`"target"`）用于「朗读中」高亮。会抢占正在进行的朗读，播完/被打断时兑现，**合成失败会 reject**（调用方必须 `.catch`，否则是 unhandled rejection） |
| `speakSequence(items)` | 依次朗读多段（`[{text,id}]`）；前一段播完再播下一段，被打断则整体中止（自动朗读原文→译文用）。**单段失败只打日志、不牵连后续段**——原文合成挂了译文该读还是要读 |
| `stopSpeaking()` | 停止当前朗读（调 `stop_speech`）并熄灭高亮/加载态 |
| `countSpeechUnits(text)` | 统计文本长度：CJK（含假名/谚文）按字计 + 其余语种按 `[\p{L}\p{N}]+` 单词计，两者相加。供「自动朗读长度上限」（`speech.auto_read_max_units`）判断 |

**关键机制：**

- **单例抢占**：全局 `playGen` 计数，每次 `speak`/`speakSequence`/`stopSpeaking` 递增；全程用 `gen === playGen` 判断是否被后来的朗读打断
  - ⚠️ **`preempt()` 不调后端的 `stop_speech`**：`speak_text` 自己就会抢占上一段，而两个 invoke 谁先到达没有保证——先发 stop 再发 speak，stop 反而可能后到、把新的这段停掉。只有明确的「停止朗读」（`stopSpeaking`）才调
- **朗读状态**：写入 `stores/ttsStore.ts` 的 `speakingId`，对应按钮显示「停止」图标
- **加载状态**：`ttsStore.loadingId` 标记「已发起合成、音频还没到」的窗口期（按钮转圈）。发起请求前置位，**收到 `{event:"start"}` 时熄灭**（后端第一段音频已送入输出设备）；`playOne` 的 `finally` 兜底清除。所有清除都带 `gen === playGen` 守卫，避免被抢占的旧会话熄掉新会话的加载态
- **无本地缓存**：音频不再回到前端，缓存只留后端那一份（`AppState.tts_cache`）

#### 为什么播放挪去了 Rust 侧

主窗口失焦会自动隐藏，WKWebView 一被标记为遮挡，WebKit 就停掉 `AudioContext` 背后的音频单元，却仍用定时器时钟继续「空转渲染」：`state` 是 `running`、`currentTime` 正常推进、分块照常排进时间线、`onended` 照常触发，**样本却没送到输出设备**，JS 侧查不出任何异常状态位。

而快捷键翻译的典型流程恰恰是「窗口弹出 → 焦点回到原 App → 窗口隐藏」，朗读几乎每次都发生在窗口隐藏期间，于是表现为「日志一切正常但一声不响」。

前端为此试过三轮（都留在 git 历史里）：

| 尝试 | 提交 | 为什么不够 |
|------|------|-----------|
| 预热输出设备（循环静音把设备拽着跑） | `1c1df3d` | 只解决设备冷启动吞开头，与窗口隐藏无关 |
| 每次朗读重建 `AudioContext` | `4844b16` | 只挡得住「朗读**前**就隐藏」，挡不住「朗读**中**隐藏」 |
| 哑火兜底（墙上时钟 vs 音频时长） | `ce78186` | 空转渲染恰恰是按实时速度走的，判据永远判定为正常 |

因此以下整套机制**已全部删除**：`StreamingPcmPlayer`、`AudioContext` 单例/重建/预热保活、`playWholeAudio` 与 `detectAudioMime`、`asArrayBuffer`、`wrapPcm16Wav`/`bytesToBase64`、前端 LRU 缓存、`primeAudio()`、`isStreamPlaybackSupported()`。

改动后音频完全不经过 WebView：窗口显示/隐藏与播放无关；而且 rodio 的样本是**被输出设备按需拉取**的，设备冷启动再慢也只是晚一点开始拉，绝不会吞掉开头。

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
  - `tts.ts` → `components/translation/ActionButtons.tsx`、`hooks/useTranslation.ts`；读 `stores/ttsStore`

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
