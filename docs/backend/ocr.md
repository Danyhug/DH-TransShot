# OCR 模块（ocr/）

## 概述

OCR 文字识别模块，通过 OpenAI 兼容的视觉语言模型 API 实现。使用共享的 `api_client` 模块发送 Chat Completions 请求。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/ocr/mod.rs` | OCR 入口，构造视觉模型请求并调用 `api_client::send_chat_completion` |

## 核心逻辑

### mod.rs

**`recognize(client, image_base64, language, base_url, api_key, model, extra) -> anyhow::Result<String>`**
1. 使用 `api_client::chat_completions_url()` 构造 API 端点
2. 构造包含图片和提示词的 Chat Completions 请求体
3. 调用 `api_client::send_chat_completion()` 发送请求（自动处理 extra 合并、Bearer auth、错误处理）
4. 返回识别到的文字内容

**注意：** `_language` 参数当前未使用（下划线前缀），提示词固定为中文。

## 依赖关系

- **内部依赖**：`api_client`（共享 HTTP 请求逻辑、ChatResponse 结构体）
- **外部依赖**：`reqwest`、`serde_json`、`log`
- **被依赖**：`commands/ocr.rs` 调用 `ocr::recognize()`

## 修改指南

- macOS FFI 代码全部 `unsafe`，修改时需特别注意内存管理（`CFRelease`）
- 新增识别语言需修改 `apple_vision.rs` 中 `lang_strs` 数组和 Windows 的语言参数
- `perform_ocr` 在独立线程运行（非 tokio 线程），因为 Vision 框架需要在非异步上下文中同步执行
- 修改图像预处理（如格式、尺寸）需同步修改 `mod.rs` 中的验证逻辑
- Windows OCR 的语言可用性取决于系统安装的 OCR 语言包
