# TTS 语音合成（tts/）

## 概述

通过 OpenAI 兼容的 `/v1/audio/speech` 接口，将文本转为语音。返回 base64 编码的音频数据（mp3），前端通过 `Audio` 元素播放。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/tts/mod.rs` | TTS 服务层：构建请求、发送 HTTP、返回 base64 音频 |

## 核心逻辑

### mod.rs

**`synthesize(client, base_url, api_key, model, extra, text) -> Result<String>`**

1. 校验 `base_url` 非空
2. 构建请求体：`{ model, input, voice: "{model}:alex", response_format: "mp3" }`
3. 调用 `merge_extra()` 合并用户自定义参数（可覆盖 `voice`、`speed`、`gain` 等）
4. 发送 `POST {base_url}/v1/audio/speech`，Bearer auth
5. 检查 HTTP 状态码，非成功则返回错误
6. 读取响应二进制数据，base64 编码后返回

**辅助函数：**
- `audio_speech_url(base_url)` — 拼接 `/v1/audio/speech` 端点 URL

## API 请求格式

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

## 依赖关系

- **依赖**：`config::merge_extra`、`reqwest::Client`、`base64`、`log`
- **被依赖**：`commands::tts::synthesize_speech`
- **与其他服务的区别**：Translation 和 OCR 使用 `api_client::send_chat_completion`（Chat Completions 端点），TTS 使用独立的 Audio Speech 端点，返回二进制音频而非 JSON

## 修改指南

- 响应格式（`response_format`）当前固定为 `mp3`，如需支持其他格式可通过 `extra` 覆盖
- 与 `api_client.rs` 共享相同的 auth 模式（Bearer token），但不共用 `send_chat_completion` 因为 TTS 端点返回二进制数据
- 日志前缀：`[TTS]`
