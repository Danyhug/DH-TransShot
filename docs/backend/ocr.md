# OCR 模块（ocr/）

## 概述

OCR 文字识别模块，通过 OpenAI 兼容的视觉语言模型 API 实现。使用共享的 `api_client` 模块发送 Chat Completions 请求。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/ocr/mod.rs` | OCR 入口，构造视觉模型请求并调用 `api_client::send_chat_completion` |

## 核心逻辑

### mod.rs

**`recognize(client, image_bytes, language, base_url, api_key, model, extra) -> anyhow::Result<String>`**
1. 使用 `api_client::chat_completions_url()` 构造 API 端点
2. 在 `spawn_blocking` 中调用图像预处理（`prepare_ocr_image_from_bytes`），避免阻塞 async 运行时
3. 对 OCR 输入图像做尺寸约束：最长边超过 `2048px` 时先缩放
4. 无透明通道时优先编码为 JPEG（减小上传体积），有透明度时保留 PNG
5. 构造包含图片和提示词的 Chat Completions 请求体
6. 调用 `api_client::send_chat_completion()` 发送请求（自动处理 extra 合并、Bearer auth、错误处理）
7. 返回识别到的文字内容

**注意：** `recognize` 接受原始图像字节（`&[u8]`），支持 JPEG/PNG 等 `image` crate 可解码的格式。调用方（如 `capture_and_ocr`）可直接传入裁切后的 JPEG 字节，避免 base64 编码/解码的往返开销。

**注意：**
- `_language` 参数当前未使用（下划线前缀），提示词固定为中文
- 当前默认不在请求体内硬编码视觉 `detail` 等 provider 扩展字段，避免破坏 OpenAI 兼容接口的兼容性；如需调优，优先通过 `ocr.extra` 注入顶层兼容参数
- 日志会记录原图/处理后分辨率、媒体类型和 base64 大小，便于判断 OCR 慢是否由大图上传导致

## 依赖关系

- **内部依赖**：`api_client`（共享 HTTP 请求逻辑、ChatResponse 结构体）
- **外部依赖**：`reqwest`、`serde_json`、`log`
- **被依赖**：`commands/ocr.rs` 调用 `ocr::recognize()`

## 修改指南

- 修改提示词（prompt）会影响识别效果，当前固定为中文提示词
- OCR 优化优先级应为：`裁切范围` > `输入尺寸` > `编码体积` > `模型参数`
- 修改图像预处理（如格式、尺寸）时，优先基于真实截图做基准测试，比较识别准确率与总耗时
- 图像缩放、转码、base64 编解码属于阻塞 CPU 工作，必须继续放在 `spawn_blocking`
- OCR 服务的 base_url / api_key / model / extra 由 `Settings` 中的 `ocr` ServiceConfig 管理
