# TTS 语音合成（tts/）

## 概述

通过 OpenAI 兼容的 TTS 接口，将文本转为语音。返回 base64 编码的音频数据（mp3），前端通过 `Audio` 元素播放。

支持两种 TTS API 格式，通过 `extra` 中的 `tts_mode` 字段切换：

| `tts_mode` | 端点 | 请求格式 | 响应格式 | 适用场景 |
|------------|------|----------|----------|----------|
| `audio_speech`（默认） | `/v1/audio/speech` | `{ model, input, voice, response_format }` | 二进制音频流 | OpenAI、SiliconFlow 等标准 TTS |
| `chat_completions` | `/v1/chat/completions` | `{ model, messages, audio: { voice, format } }` | JSON，音频 base64 嵌在 `choices[0].message.audio.data` | 小米 MiMo 等 Chat Completions 格式 TTS |

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/tts/mod.rs` | TTS 服务层：模式判断、构建请求、发送 HTTP、返回 base64 音频 |

## 核心逻辑

### mod.rs

**`synthesize(client, base_url, api_key, model, extra, text) -> Result<String>`**

1. 校验 `base_url` 非空
2. 从 `extra` 中读取 `tts_mode` 判断请求模式（默认 `audio_speech`）
3. 根据模式分发到对应的合成函数

**`synthesize_via_audio_speech`（audio_speech 模式）**

1. 构建请求体：`{ model, input, voice: "{model}:alex", response_format: "mp3" }`
2. 调用 `merge_extra()` 合并用户自定义参数
3. 移除 `tts_mode` 内部字段（不发送给 API）
4. 发送 `POST {base_url}/v1/audio/speech`，Bearer auth
5. 读取响应二进制数据，base64 编码后返回

**`synthesize_via_chat_completions`（chat_completions 模式，小米 MiMo 等）**

1. 构建请求体：
   ```json
   {
     "model": "...",
     "messages": [{ "role": "assistant", "content": "文本" }],
     "audio": { "voice": "mimo_default", "format": "mp3" },
     "modalities": ["text", "audio"]
   }
   ```
2. 调用 `merge_extra()` 合并用户自定义参数（可覆盖 `voice`、`audio.format` 等）
3. 移除 `tts_mode` 内部字段
4. 发送 `POST {base_url}/v1/chat/completions`，Bearer auth
5. 解析 JSON 响应，提取 `choices[0].message.audio.data`（base64 音频）返回

### commands/tts.rs

**`synthesize_speech(state, text) -> Result<String, String>`**

1. 从 `AppState.settings` 读取当前 TTS 配置
2. 使用 `base_url + model + extra + text` 生成缓存键
   - 文本在生成缓存键和请求前会先做规范化：`trim()` + `CRLF -> LF`
3. 先查询 `AppState.tts_cache`
4. 命中时直接返回缓存的 base64 音频，不发起网络请求
5. 未命中时调用 `tts::synthesize()`
6. 成功后写入缓存，供后续重复朗读复用

缓存当前保存在内存中，最大 64 条，超出后按插入顺序淘汰旧项。保存设置时会清空缓存，避免模型、voice 或其它参数变化后继续复用旧音频。

**辅助函数：**
- `resolve_tts_mode(extra)` — 从 `extra` JSON 中读取 `tts_mode` 字段，返回 `"audio_speech"` 或 `"chat_completions"`
- `audio_speech_url(base_url)` — 复用 `api_client::build_endpoint_url(base_url, "audio/speech")` 自适应拼接端点

## API 请求格式

### audio_speech 模式（默认）

```json
{
  "model": "IndexTeam/IndexTTS-2",
  "input": "要朗读的文本",
  "voice": "IndexTeam/IndexTTS-2:alex",
  "response_format": "mp3"
}
```

默认 voice 为 `{model}:alex`。用户可通过 `extra` 字段覆盖 voice 或添加其他参数：

```json
{
  "voice": "IndexTeam/IndexTTS-2:bella",
  "speed": 1.2
}
```

可用 voice 名称（以 SiliconFlow 为例）：alex, anna, bella, benjamin, charles, claire, david, diana。格式为 `{model}:{name}`。

### chat_completions 模式（小米 MiMo 等）

`extra` 配置示例：

```json
{
  "tts_mode": "chat_completions",
  "voice": "mimo_default",
  "audio": {
    "format": "mp3"
  }
}
```

请求体会自动构建为：

```json
{
  "model": "mimo-tts",
  "messages": [{ "role": "assistant", "content": "要朗读的文本" }],
  "audio": { "voice": "mimo_default", "format": "mp3" },
  "modalities": ["text", "audio"]
}
```

小米 MiMo 可用 voice：`mimo_default`、`default_zh`、`default_en` 等（直接填音色名，无需 `model:` 前缀）。

## 依赖关系

- **依赖**：`config::merge_extra`、`api_client::chat_completions_url`、`api_client::build_endpoint_url`、`reqwest::Client`、`base64`、`log`、`serde`/`serde_json`
- **被依赖**：`commands::tts::synthesize_speech`
- **与其他服务的区别**：Translation 和 OCR 使用 `api_client::send_chat_completion`（Chat Completions 端点）；TTS 的 `audio_speech` 模式使用独立的 Audio Speech 端点返回二进制音频，`chat_completions` 模式使用 Chat Completions 端点但解析 `message.audio.data` 而非 `message.content`

## 修改指南

- `tts_mode` 是内部控制字段，会在发送请求前从请求体中移除，不会出现在 API 请求中
- 响应格式（`response_format` / `audio.format`）当前默认为 `mp3`，如需支持其他格式可通过 `extra` 覆盖
- 与 `api_client.rs` 共享相同的 auth 模式（Bearer token）
- TTS 缓存键依赖 `base_url`、`model`、`extra`、`text`；`tts_mode` 包含在 `extra` 中，所以切换模式会自动使旧缓存失效
- 当前缓存为进程内内存缓存，应用重启后失效；如果后续需要跨启动复用，再引入磁盘缓存
- 日志前缀：`[TTS]`
