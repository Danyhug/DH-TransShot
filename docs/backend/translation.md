# 翻译模块（translation/）

## 概述

基于 OpenAI 兼容接口的 LLM 翻译模块，支持任何兼容 OpenAI Chat Completions API 的服务（OpenAI、DeepSeek、Ollama 等）。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/translation/mod.rs` | 模块声明，公开导出 `OpenAiCompatProvider` |
| `src-tauri/src/translation/provider.rs` | Translation Provider trait（预留扩展，当前未使用） |
| `src-tauri/src/translation/openai_compat.rs` | OpenAI 兼容 Chat Completions 客户端实现 |

## 核心逻辑

### openai_compat.rs

**`OpenAiCompatProvider`**
- 持有 `reqwest::Client` 实例用于 HTTP 请求

**`translate(text, source_lang, target_lang, base_url, api_key, model, extra) -> anyhow::Result<String>`**

1. **构造系统提示词：**
   ```
   You are a translator. Translate the following text from {source} to {target}.
   Output ONLY the translated text, nothing else.
   Do not add explanations, notes, or any extra content.
   ```
   - 若 `source_lang == "auto"` 则使用 "the detected language"

2. **构造请求体：**
   - messages：system prompt + user text
   - temperature：0.3（低随机性）

3. **调用 `api_client::send_chat_completion()`：**
   - 自动处理 URL 拼接、extra 合并、Bearer auth、HTTP 错误检查、响应解析
   - 返回 `choices[0].message.content` 并 trim

## 依赖关系

- **外部依赖**：`reqwest`（HTTP 客户端）、`serde_json`（序列化）、`log`
- **内部依赖**：`api_client`（共享 HTTP 请求逻辑、ChatResponse 结构体）
- **被依赖**：`commands/translation.rs` 创建 `OpenAiCompatProvider` 实例并调用 `translate()`

## 修改指南

- `temperature: 0.3` 为翻译场景优化的值，调高会增加输出随机性
- 系统提示词直接影响翻译质量，修改时需充分测试不同语言对
- `base_url` 末尾的 `/` 由 `api_client` 自动去除
- 空 `api_key` 时不发送 Authorization header（适配 Ollama 等本地服务）
- 如需支持流式翻译，需将 HTTP 响应改为 SSE 流处理
- 新增翻译 Provider（如直接调用 Google Translate API）可实现 `provider.rs` 中的 trait
