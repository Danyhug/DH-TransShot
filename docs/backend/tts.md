# TTS 语音合成（tts/）

## 概述

将文本转为语音，返回 **base64 编码的音频**，前端通过 `Audio` 元素播放。

根据用户配置的 TTS 端点**自适应选择两种协议**：

| 协议 | 触发条件 | 端点 | 请求体 | 响应 |
|------|---------|------|--------|------|
| **audio/speech** | 端点非 `chat/completions`（默认） | `/v1/audio/speech` | `{model, input, voice, response_format}` | 二进制音频 |
| **chat+audio** | 端点以 `chat/completions` 结尾 | `/v1/chat/completions` | `{model, messages, audio:{format,voice}}` | JSON，音频在 `choices[0].message.audio.data`（base64） |

- audio/speech：OpenAI 兼容标准 TTS（如 SiliconFlow `FunAudioLLM/CosyVoice2`）
- chat+audio：小米 MiMo 式「聊天补全 + 音频输出」TTS（如 `mimo-v2.5-tts`），文本走 `messages`、音色走 `audio` 对象

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/tts/mod.rs` | TTS 服务层：解析端点、按协议构建请求、发送 HTTP、返回 base64 音频 |

## 核心逻辑

### mod.rs

**`synthesize(client, base_url, api_key, model, extra, text) -> Result<String>`（协议分发入口）**

1. 校验 `base_url` 非空
2. `resolve_tts_endpoint_url(base_url)` 解析端点 URL（复用 `build_endpoint_url`，`endpoint="audio/speech"`）
3. `is_chat_audio_endpoint(url)` 判定：URL 去掉 query/尾斜杠后以 `chat/completions` 结尾 → 走 chat+audio，否则走 audio/speech
4. 两条路径最终都返回「base64 音频字符串」

**`synthesize_audio_speech(...)` — 标准协议**

1. 构建请求体：`{ model, input, voice: "{model}:alex", response_format: "mp3" }`
2. `merge_extra()` **整包合并** `extra`（可覆盖 `voice`、`speed`、`gain`、`response_format` 等）
3. `POST` → 检查状态码 → 读取二进制 → base64 编码返回

**`synthesize_chat_audio(...)` — 小米 chat+audio 协议**

1. 解析 `extra`（JSON），**只解释三个键**（不整包合并，避免 audio/speech 字段污染 chat 请求体）：
   - `voice`：音色裸名字，默认 `Chloe`；**忽略含 `/` 的 model-scoped 音色**（如 `FunAudioLLM/...:alex`，那是另一协议遗留值）
   - `format`：音频格式，默认 `wav`（仅认 `format` 键，不复用 `response_format`）
   - `style`：风格指令（`user` 消息），默认中性提示；置为空串则不发送 `user` 消息
2. 组织 `messages`（官方示例约定）：`user`=风格指令，`assistant`=要朗读的文本
3. 构建请求体：`{ model, messages, audio: { format, voice } }`
4. `POST` → 检查状态码 → 解析 JSON，取 `choices[0].message.audio.data`（**已是 base64，直接返回**）

**辅助函数：**
- `resolve_tts_endpoint_url(base_url)` — `build_endpoint_url(base_url, "audio/speech")` 自适应拼接（根/版本段/完整端点/`#` raw，规则见 [config.md](config.md)）。用户用 `#` raw 或完整路径指向 `.../chat/completions` 时原样返回该 chat 端点
- `is_chat_audio_endpoint(url)` — 端点是否为 Chat Completions（决定走哪条协议）

### commands/tts.rs

**`synthesize_speech(state, text) -> Result<String, String>`**

1. 从 `AppState.settings` 读取当前生效 TTS 配置（`tts.resolved(...)` 按 `active` 选默认或某个 provider）
2. 使用 `base_url + model + extra + text` 生成缓存键
   - 文本先规范化：`trim()` + `CRLF -> LF`
3. 命中 `AppState.tts_cache` 直接返回缓存 base64；未命中调用 `tts::synthesize()`，成功后写入缓存

缓存为进程内内存缓存，最大 64 条，按插入顺序淘汰。保存设置时清空缓存。缓存键与协议无关（不同协议因 base_url 不同天然不撞键）。

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

```json
{
  "model": "mimo-v2.5-tts",
  "messages": [
    { "role": "user", "content": "用自然、平稳、清晰的语气朗读。" },
    { "role": "assistant", "content": "要朗读的文本" }
  ],
  "audio": { "format": "wav", "voice": "Chloe" }
}
```

响应（音频在 `choices[0].message.audio.data`，base64）：

```json
{ "choices": [ { "message": { "audio": { "data": "<base64 音频>" } } } ] }
```

通过 `extra` 自定义（仅这三个键生效）：

```json
{ "voice": "Chloe", "format": "wav", "style": "低沉沙哑，像历经沧桑的老前辈娓娓道来。" }
```

## 小米 MiMo TTS 配置指引

在设置里为 TTS 增加一个 provider（或改默认服务的 base_url）：

- **base_url**：`https://api.xiaomimimo.com/v1/chat/completions#`
  - 结尾的 `#` 是 **raw 标记**，强制原样请求该 chat 端点（否则会被自适应规则错误拼成 `.../v1/audio/speech`）
- **model**：`mimo-v2.5-tts`
- **api_key**：小米控制台生成的 Key
- **`tts.extra` 的 voice 必须是小米音色裸名字**（如 `Chloe`）：若沿用硅基流动的 `FunAudioLLM/...:alex`（含 `/`）会被自动忽略并回退默认音色

> ⚠️ `tts.extra` 是整个 TTS 服务共享的（默认 provider 与各 provider 共用）。若同时在用两种协议的 provider，注意 extra 里 audio/speech 专用字段（`speed`/`sample_rate`/`response_format`）不会进入 chat+audio 请求体，反之 chat+audio 的 `format`/`style` 对 audio/speech 也无意义。

## 前端播放（音频格式自适应）

后端返回的 base64 音频**格式不定**（audio/speech 为 mp3，chat+audio 默认 wav）。`components/translation/ActionButtons.tsx` 的 `detectAudioMime()` 按音频魔数（`RIFF`→wav、`ID3`/帧同步→mp3、`OggS`→ogg、`fLaC`→flac）嗅探 MIME，再拼 `data:{mime};base64,...` 播放，**不写死 mp3**，保证 wav 也能播放。

## 依赖关系

- **依赖**：`config::merge_extra`（仅 audio/speech 路径）、`api_client::build_endpoint_url`、`reqwest::Client`、`serde`/`serde_json`、`base64`、`log`
- **被依赖**：`commands::tts::synthesize_speech`
- **与其他服务的区别**：Translation/OCR 固定走 Chat Completions 并解析文本；TTS 按端点在「二进制 audio/speech」与「base64-in-JSON 的 chat+audio」两种协议间自适应，都不复用 `send_chat_completion`

## 修改指南

- **新增 TTS 协议**时，在 `synthesize()` 的分发处增加判定分支，并新增独立的 `synthesize_xxx()` 函数；避免把不同协议的字段混进同一请求体
- chat+audio 路径**故意不 `merge_extra`**：其请求体是 chat 结构，audio/speech 的 `response_format`/`speed` 等会报错或被误解；如需暴露更多 chat 参数，显式解析特定键或引入白名单合并
- `messages` 结构（`user`=风格指令 / `assistant`=文本）照搬小米官方示例；若实测需调整（如单条 user、或加 `modalities`），改 `synthesize_chat_audio` 即可
- 默认音色/格式/风格由 `DEFAULT_CHAT_AUDIO_VOICE`/`DEFAULT_CHAT_AUDIO_FORMAT`/`DEFAULT_CHAT_AUDIO_STYLE` 常量控制
- audio/speech 的 `response_format` 仍固定默认 `mp3`，可经 `extra` 覆盖
- 新增会影响音频输出的默认字段时，须同步纳入缓存键（`commands/tts.rs`）
- 前端播放依赖 `detectAudioMime`，新增音频格式时补充对应魔数
- 日志前缀：`[TTS]`；日志中会带上 `协议=audio/speech|chat+audio`、`voice`、`format`、尺寸/耗时，便于排查
