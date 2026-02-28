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
- 关闭所有已有的 `screenshot-overlay-*` 覆盖层窗口
- 关闭 settings 和 debug-log 子窗口（避免遮挡覆盖层）
- 采集窗口矩形列表（`list_window_rects()`），存入 `AppState.frozen_window_rects`
- 收集所有显示器信息（`MonitorInfo`：名称、物理坐标、物理尺寸、scale_factor）
- 计算每个显示器的逻辑矩形，调用 `capture_monitors()` 逐显示器截图
- 将逐显示器截图（`Vec<String>`）存入 `AppState.frozen_screenshots`
- 将显示器信息列表存入 `AppState.frozen_monitors`
- 为每个显示器创建一个覆盖层窗口（label: `screenshot-overlay-0`, `screenshot-overlay-1`, ...）：
  - 位置和尺寸对应该显示器
  - 无边框、置顶、跳过任务栏、初始隐藏（前端加载完成后显示）
- 主覆盖层（overlay-0）关闭时根据 mode 决定行为：
  - `screenshot` 模式：不做额外操作
  - `ocr_translate` 模式：show + focus 主窗口
- 监听 `close-all-overlays` 事件，关闭所有覆盖层窗口

**`get_frozen_screenshot(state, monitor_index) -> Result<serde_json::Value, String>`**
- 参数 `monitor_index` 指定要获取哪个显示器的截图
- 返回 JSON 对象含：`image`（该显示器的 base64 PNG）、`mode`、`window_rects`、`monitors`（显示器信息列表）

**`capture_region(state, monitor_index, x, y, width, height) -> Result<String, String>`**
- 从 `AppState.frozen_screenshots[monitor_index]` 取出该显示器的冻结截图
- 调用 `screenshot::capture_region_from_full()` 裁切指定区域
- 坐标为该显示器图像的像素坐标（前端已将 CSS 坐标 × DPR 转换为图像像素）

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
- `get_frozen_screenshot` 和 `capture_region` 均需要 `monitor_index` 参数来定位显示器
