# TTS 语音合成（tts/）

## 概述

将文本转为语音，返回 **base64 编码的完整音频**，前端通过 `Audio` 元素播放；chat+audio 流式路径额外通过 **IPC Channel** 把 PCM 分块以二进制实时推给前端边收边播。

根据用户配置的 TTS 端点**自适应选择两种协议**：

| 协议 | 触发条件 | 端点 | 请求体 | 响应 |
|------|---------|------|--------|------|
| **audio/speech** | 端点非 `chat/completions`（默认） | `/v1/audio/speech` | `{model, input, voice, response_format}` | 二进制音频 |
| **chat+audio（非流式）** | 端点以 `chat/completions` 结尾，`extra.stream=false` | `/v1/chat/completions` | `{model, messages, audio:{format,voice}}` | JSON，音频在 `choices[0].message.audio.data`（base64） |
| **chat+audio（流式，默认）** | 端点以 `chat/completions` 结尾 | `/v1/chat/completions` | `{model, messages, audio:{format:"pcm16",voice}, stream:true}` | SSE，逐块 `choices[0].delta.audio.data`（base64 PCM16LE） |

- audio/speech：OpenAI 兼容标准 TTS（如 SiliconFlow `FunAudioLLM/CosyVoice2`）
- chat+audio：小米 MiMo 式「聊天补全 + 音频输出」TTS（如 `mimo-v2.5-tts`），文本走 `messages`、音色走 `audio` 对象
- chat+audio **默认走 SSE 流式**（`stream:true` + `pcm16`），分块实时回传前端播放；结束后把所有 PCM 拼成 WAV 返回（供缓存/重播）。可用 `extra.stream=false` 关掉流式

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/tts/mod.rs` | TTS 服务层：解析端点、按协议构建请求、发送 HTTP、返回 base64 音频 |

## 核心逻辑

### mod.rs

**`synthesize(client, base_url, api_key, model, extra, text, on_chunk) -> Result<SpeechResult>`（协议分发入口）**

1. 校验 `base_url` 非空
2. `resolve_tts_endpoint_url(base_url)` 解析端点 URL（复用 `build_endpoint_url`，`endpoint="audio/speech"`）
3. `is_chat_audio_endpoint(url)` 判定：URL 去掉 query/尾斜杠后以 `chat/completions` 结尾 → 走 chat+audio，否则走 audio/speech
4. 两条路径最终都返回 `SpeechResult`

**`SpeechResult` — 合成结果**

| 字段 | 说明 |
|------|------|
| `audio_base64` | 完整音频 base64（流式路径为 PCM 拼接后套 WAV 头） |
| `chunk_count` | 已推送给前端的流式分块数；`0` 表示未走流式分块（命中缓存/非 chat+audio/服务端未流式） |
| `sample_rate` | 流式分块采样率（Hz），非流式为 0 |

**`ChunkSink<'a>` / `on_chunk`** — 流式分块回调 `Fn(&[u8])`，参数是**已解码的 PCM16LE 裸字节**（不是 base64）；命令层把它原样写进 IPC Channel 的二进制消息。`None` 时仍会累积拼成完整音频，只是不实时回调。

**`synthesize_audio_speech(...)` — 标准协议**

1. 构建请求体：`{ model, input, voice: "{model}:alex", response_format: "mp3" }`
2. `merge_extra()` **整包合并** `extra`（可覆盖 `voice`、`speed`、`gain`、`response_format` 等）
3. `POST` → 检查状态码 → 读取二进制 → base64 编码，包成 `SpeechResult::whole`（`chunk_count=0`）

**`synthesize_chat_audio(...)` — 小米 chat+audio 协议入口**

1. `parse_chat_audio_options(extra)` 解析 `extra`（JSON），**只解释四个键**（不整包合并，避免 audio/speech 字段污染 chat 请求体）：
   - `voice`：音色裸名字，默认 `Milo`；**忽略含 `/` 的 model-scoped 音色**（如 `FunAudioLLM/...:alex`，那是另一协议遗留值）
   - `format`：**非流式**音频格式，默认 `wav`（仅认 `format` 键，不复用 `response_format`）
   - `style`：风格指令（`user` 消息），默认中性提示；置为空串则不发送 `user` 消息
   - `stream`：是否走 SSE 流式，默认 `true`
2. `stream=true` → 先试 `synthesize_chat_audio_stream`，失败降级 `synthesize_chat_audio_once`；`stream=false` → 直接非流式

**`synthesize_chat_audio_once(...)` — chat+audio 非流式**

1. 组织 `messages`（`build_chat_audio_messages`）：`user`=风格指令，`assistant`=要朗读的文本
2. 构建请求体：`{ model, messages, audio: { format, voice } }`
3. `POST` → 检查状态码 → 解析 JSON，取 `choices[0].message.audio.data`（**已是 base64，直接返回**）

**`synthesize_chat_audio_stream(...)` — chat+audio 流式（默认）**

1. 请求体额外带 `stream:true`，音频格式**固定 `pcm16`**（官方要求，只有裸 PCM 分块能直接拼接）
2. 用 `response.bytes_stream()` 逐块读取，按行解析 SSE（`data: {...}`）：
   - `extract_stream_audio_data` 从 `choices[0].delta.audio.data`（兼容末块 `message.audio.data`）取 base64 PCM
   - 每块 base64 解码后**先** `on_chunk(&bytes)` 实时回传前端、再累积到 PCM 缓冲（首帧延迟优先）；跨块残留的行/字节留到下次
   - 收到 `error` 字段立即 bail
3. 若整个响应**不含任何 `data:` 行**（服务端不支持 stream 的兼容端点）→ 整体当普通 JSON 解析，退回非流式结果
4. 结束后 `wrap_pcm16_wav(pcm, 24000, 1)` 套 44 字节 WAV 头 → base64，返回 `SpeechResult{ audio_base64, chunk_count, sample_rate:24000 }`

**辅助函数：**
- `resolve_tts_endpoint_url(base_url)` — `build_endpoint_url(base_url, "audio/speech")` 自适应拼接（根/版本段/完整端点/`#` raw，规则见 [config.md](config.md)）。用户用 `#` raw 或完整路径指向 `.../chat/completions` 时原样返回该 chat 端点
- `is_chat_audio_endpoint(url)` — 端点是否为 Chat Completions（决定走哪条协议）
- `stream_sample_rate()` / `stream_channels()` — 暴露流式 PCM 参数常量（24000 / 1）给命令层填入 `start` 控制消息
- `wrap_pcm16_wav(pcm, sample_rate, channels)` — 给裸 PCM16LE 套标准 WAV 头
- `extract_stream_audio_data(value)` — 从一条 SSE 分块里取音频 base64（优先 `delta`，兼容 `message`）

### commands/tts.rs

两个命令共用 `synthesize_inner`（规范化文本 → 解析配置 → 查缓存 → 合成 → 写缓存），区别只在是否传 `on_chunk`：

**`synthesize_speech(state, text) -> Result<String, String>`** — 非流式回调，返回完整音频 base64（前端 `synthesizeSpeech`）。

**`synthesize_speech_stream(state, text, on_chunk) -> Result<SpeechResponse, String>`** — 边收边播版本（前端 `synthesizeSpeechStream`）：
- `on_chunk: Channel<InvokeResponseBody>` 是前端传入的 **IPC Channel**（不是全局事件）：
  - 合成前先发一条 JSON 控制消息 `{"event":"start","sampleRate":24000,"channels":1}`
  - 每个分块以 `InvokeResponseBody::Raw(pcm)` 二进制发送（前端收到 `ArrayBuffer`）
  - 合成结束后发 `{"event":"end","chunkCount":N}`
- 通道消息由 Tauri 保证按发送顺序投递，`end` 一定排在所有分块之后，前端无需比对分块计数
- 返回 `SpeechResponse{ audio, chunkCount, sampleRate }`（camelCase）：`chunkCount == 0` 时 `audio` 是完整音频、前端直接播；**`chunkCount > 0` 时 `audio` 被清空**（音频已逐块送达，再回传一份完整 WAV 会让长文本白白多传数 MB），前端重播时靠后端缓存

> **为什么用 Channel 而不是 `app.emit`**：`app.emit` 会把负载 JSON 拼进 `eval` 脚本字符串，广播给**每一个** webview（本项目有 main/screenshot/debug/settings 四个），且必须在主线程逐条执行。长文本几百个 base64 分块会把主线程堵死，表现为「必须等整段流传完才开始播、长文本干脆播不出来」。Channel 只投递给发起调用的 webview，且大于 1KB 的二进制走 IPC 自定义协议（fetch），不进 eval 字符串。

**`synthesize_inner` 流程：**
1. 从 `AppState.settings` 读取当前生效 TTS 配置（`tts.resolved(...)` 按 `active` 选默认或某个 provider）
2. 使用 `base_url + model + extra + text` 生成缓存键（文本先规范化：`trim()` + `CRLF -> LF`）
3. 命中 `AppState.tts_cache` 直接返回缓存 base64（`chunk_count=0`，不推流式分块）；未命中调用 `tts::synthesize()`，成功后写入缓存

缓存为进程内内存缓存，最大 64 条，按插入顺序淘汰。保存设置时清空缓存。缓存键与协议无关（不同协议因 base_url 不同天然不撞键）。缓存存的始终是「完整音频 base64」，命中时不再推流式分块，前端整段播放。

## 前端通道与播放

- 分块走 IPC Channel：控制消息为 JSON（`{event:"start"|"end", ...}`），音频分块为二进制 `ArrayBuffer`（PCM16LE 单声道）
- 前端 `lib/tts.ts` 的 `StreamingPcmPlayer` 逐块解码 PCM16→Float32，调度进共享 `AudioContext` 无缝排布；首块预留 0.2s 缓冲吸收网络抖动，收到 `end` 且全部播完时结束
- 详见 [docs/frontend/lib.md](../frontend/lib.md)

## API 请求格式

### audio/speech 协议

```json
{
  "model": "FunAudioLLM/CosyVoice2-0.5B",
  "input": "要朗读的文本",
  "voice": "FunAudioLLM/CosyVoice2-0.5B:alex",
  "response_format": "mp3"
}
```

默认 voice 为 `{model}:alex`，可用 voice（以 SiliconFlow 为例）：alex, anna, bella, benjamin, charles, claire, david, diana。可通过 `extra` 覆盖 `voice`/`speed` 等。

### chat+audio 协议（小米 MiMo）

**非流式**（`extra.stream=false`）：

```json
{
  "model": "mimo-v2.5-tts",
  "messages": [
    { "role": "user", "content": "用自然、平稳、清晰的语气朗读。" },
    { "role": "assistant", "content": "要朗读的文本" }
  ],
  "audio": { "format": "wav", "voice": "Milo" }
}
```

响应（音频在 `choices[0].message.audio.data`，base64）：

```json
{ "choices": [ { "message": { "audio": { "data": "<base64 音频>" } } } ] }
```

**流式**（默认，`stream:true` + `pcm16`）：

```json
{
  "model": "mimo-v2.5-tts",
  "messages": [ /* 同上 */ ],
  "audio": { "format": "pcm16", "voice": "Milo" },
  "stream": true
}
```

SSE 响应逐块（PCM16LE / 24kHz / 单声道，需自行拼接）：

```
data: {"choices":[{"delta":{"audio":{"data":"<base64 PCM 分块>"}}}]}
data: [DONE]
```

**内置音色**（`mimo-v2.5-tts`，填裸名字）：`mimo_default`（随集群，中国区=冰糖/其它=Mia）、`冰糖`/`茉莉`（中文女）、`苏打`/`白桦`（中文男）、`Mia`/`Chloe`（英文女）、`Milo`/`Dean`（英文男）。默认 `Milo`。

通过 `extra` 自定义（仅这四个键生效）：

```json
{ "voice": "Milo", "format": "wav", "style": "低沉沙哑，像历经沧桑的老前辈娓娓道来。", "stream": true }
```

## 小米 MiMo TTS 配置指引

在设置里为 TTS 增加一个 provider（或改默认服务的 base_url）：

- **base_url**：`https://api.xiaomimimo.com/v1/chat/completions#`
  - 结尾的 `#` 是 **raw 标记**，强制原样请求该 chat 端点（否则会被自适应规则错误拼成 `.../v1/audio/speech`）
- **model**：`mimo-v2.5-tts`
- **api_key**：小米控制台生成的 Key
- **`tts.extra` 的 voice 必须是小米音色裸名字**（如 `Milo`、`冰糖`）：若沿用硅基流动的 `FunAudioLLM/...:alex`（含 `/`）会被自动忽略并回退默认音色 `Milo`
- **流式默认开启**：`extra` 不填 `stream` 即走 SSE 流式（`pcm16`）；如需关掉填 `{"stream": false}`。「边收边播」还受前端 `speech.stream_playback` 开关控制

> ⚠️ `tts.extra` 是整个 TTS 服务共享的（默认 provider 与各 provider 共用）。若同时在用两种协议的 provider，注意 extra 里 audio/speech 专用字段（`speed`/`sample_rate`/`response_format`）不会进入 chat+audio 请求体，反之 chat+audio 的 `format`/`style`/`stream` 对 audio/speech 也无意义。

## 前端播放（音频格式自适应）

后端返回的 base64 音频**格式不定**（audio/speech 为 mp3，chat+audio 非流式默认 wav，流式为拼接后的 wav）。`lib/tts.ts` 的 `detectAudioMime()` 按音频魔数（`RIFF`→wav、`ID3`/帧同步→mp3、`OggS`→ogg、`fLaC`→flac）嗅探 MIME，再拼 `data:{mime};base64,...` 播放整段，**不写死 mp3**，保证 wav 也能播放。流式分块则走 `StreamingPcmPlayer`（Web Audio）边收边播，见 [docs/frontend/lib.md](../frontend/lib.md)。

## 依赖关系

- **依赖**：`config::merge_extra`（仅 audio/speech 路径）、`api_client::build_endpoint_url`、`reqwest::Client`（含 `stream` feature）、`futures_util::StreamExt`、`serde`/`serde_json`、`base64`、`log`
- **被依赖**：`commands::tts::synthesize_speech`、`commands::tts::synthesize_speech_stream`
- **与其他服务的区别**：Translation/OCR 固定走 Chat Completions 并解析文本；TTS 按端点在「二进制 audio/speech」与「chat+audio（流式 SSE / 非流式 JSON）」间自适应，都不复用 `send_chat_completion`

## 修改指南

- **新增 TTS 协议**时，在 `synthesize()` 的分发处增加判定分支，并新增独立的 `synthesize_xxx()` 函数；避免把不同协议的字段混进同一请求体
- chat+audio 路径**故意不 `merge_extra`**：其请求体是 chat 结构，audio/speech 的 `response_format`/`speed` 等会报错或被误解；如需暴露更多 chat 参数，在 `parse_chat_audio_options` 里显式解析特定键
- `messages` 结构（`user`=风格指令 / `assistant`=文本）照搬小米官方示例；若实测需调整（如单条 user、或加 `modalities`），改 `build_chat_audio_messages` 即可
- 默认音色/格式/风格由 `DEFAULT_CHAT_AUDIO_VOICE`(`Milo`)/`DEFAULT_CHAT_AUDIO_FORMAT`(`wav`)/`DEFAULT_CHAT_AUDIO_STYLE` 常量控制；流式采样率/声道由 `CHAT_AUDIO_STREAM_SAMPLE_RATE`(24000)/`CHAT_AUDIO_STREAM_CHANNELS`(1)
- audio/speech 的默认音色仍是 `{model}:alex`、`response_format` 默认 `mp3`，均可经 `extra` 覆盖
- **流式格式固定 `pcm16`**：小米流式只有 PCM 裸块能拼接，`wrap_pcm16_wav` 依赖 24kHz/单声道假设；若上游改采样率需同步 `CHAT_AUDIO_STREAM_SAMPLE_RATE` 和事件里的 `sampleRate`
- 新增会影响音频输出的默认字段时，须同步纳入缓存键（`commands/tts.rs`）
- 前端整段播放依赖 `detectAudioMime`，新增音频格式时补充对应魔数
- 日志前缀：`[TTS]`；日志中会带上 `协议=audio/speech|chat+audio(流式/非流式)`、`voice`、`format`、分块数、首块/总耗时，便于排查
