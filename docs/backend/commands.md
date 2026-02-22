# 命令层（commands/）

## 概述

Tauri 命令层，作为前后端 RPC 接口，将前端的 `invoke()` 调用路由到对应的后端功能模块。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/commands/mod.rs` | 模块声明，公开导出 6 个子模块 |
| `src-tauri/src/commands/screenshot.rs` | 截图相关命令（区域选择、区域裁切、获取冻结截图） |
| `src-tauri/src/commands/ocr.rs` | OCR 识别命令 |
| `src-tauri/src/commands/translation.rs` | LLM 翻译命令 |
| `src-tauri/src/commands/settings.rs` | 设置读写命令 |
| `src-tauri/src/commands/tts.rs` | TTS 语音合成命令 |

## 核心逻辑

### screenshot.rs

**`start_region_select(app, state, mode) -> Result<(), String>`**
- 读取 `hide_on_capture` 设置
- 关闭 settings 和 debug-log 子窗口（避免遮挡覆盖层）
- 仅 `hide_on_capture == true` 时将主窗口移至屏幕外
- 捕获全屏截图并存入 `AppState.frozen_screenshot`
- 获取主显示器物理尺寸和缩放因子，计算逻辑尺寸
- 创建覆盖层窗口（`screenshot.html`）：全屏、无边框、置顶、跳过任务栏
- 覆盖层关闭时根据 mode 和是否移走了主窗口决定恢复行为：
  - `screenshot` 模式：仅恢复主窗口位置（如果之前移走了）
  - `ocr_translate` 模式：恢复位置 + show + focus

**`capture_region(state, x, y, width, height) -> Result<String, String>`**
- 从 `AppState.frozen_screenshot` 取出冻结截图
- 调用 `screenshot::capture_region_from_full()` 裁切指定区域
- 坐标为物理像素（前端已乘以 DPR）

### ocr.rs

**`recognize_text(state, image_base64, language) -> Result<String, String>`**
- 调用 `ocr::recognize()` 进行 OCR 识别
- 平台无关的统一接口

### translation.rs

**`translate_text(state, text, source_lang, target_lang) -> Result<String, String>`**
- 从 `AppState.settings` 读取 LLM 配置（base_url、api_key、model）
- 创建 `OpenAiCompatProvider` 实例执行翻译
- Mutex 锁的作用域尽量小，取完配置即释放

### settings.rs

**`get_settings(state) -> Result<Settings, String>`**
- 返回 `AppState.settings` 的克隆

**`save_settings(state, settings) -> Result<(), String>`**
- 替换 `AppState.settings` 的内容
- 当前仅内存持久化，重启丢失

### tts.rs

**`synthesize_speech(state, text) -> Result<String, String>`**
- 从 `AppState.settings` 读取 TTS 配置（base_url、api_key、tts.model、tts.extra）
- 调用 `tts::synthesize()` 发送请求到 `/v1/audio/speech`
- 返回 base64 编码的 mp3 音频数据
- Mutex 锁的作用域尽量小，取完配置即释放

## 依赖关系

- **依赖**：`config::AppState`、`config::Settings`、`screenshot`、`ocr`、`translation::OpenAiCompatProvider`、`tts`
- **被依赖**：`lib.rs` 中通过 `generate_handler!` 注册
- **前端对应**：`src/lib/invoke.ts` 中的类型化封装函数

## 修改指南

- 所有命令使用 `#[tauri::command]` 宏标注，返回 `Result<T, String>`
- 新增命令后需在 `lib.rs` 的 `generate_handler!` 中注册，同时在前端 `invoke.ts` 添加对应封装
- `AppState` 的 Mutex 锁应尽量缩小作用域，避免跨 await 持锁
- `start_region_select` 中的窗口创建逻辑涉及 DPI 转换，修改时参考 `docs/architecture.md` 的 DPI 处理说明
