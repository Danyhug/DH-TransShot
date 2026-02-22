# 截图模块（screenshot/）

## 概述

封装屏幕截图功能，提供全屏捕获、区域裁切和窗口矩形列表采集，输出 base64 编码的 PNG 图片。macOS 使用 Core Graphics FFI 直接调用系统 API。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/screenshot/mod.rs` | 模块声明，公开导出 `capture_full`、`capture_region_from_full`、`list_window_rects`、`WindowRect` |
| `src-tauri/src/screenshot/capture.rs` | 截图逻辑实现 + 窗口矩形列表采集 |

## 核心逻辑

### capture.rs

**`WindowRect`** 结构体
- 表示一个可见窗口的矩形区域（逻辑坐标 / points）
- 字段：`x`, `y`, `width`, `height`（均为 `f64`）
- 实现 `serde::Serialize`，可直接序列化为 JSON

**`list_window_rects() -> Vec<WindowRect>`**
- macOS：通过 `CGWindowListCopyWindowInfo` FFI 获取所有可见窗口
  - 选项：`kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements`（= 17）
  - 过滤条件：`kCGWindowLayer == 0`（仅普通窗口）、`kCGWindowIsOnscreen == true`、尺寸 >= 10×10
  - 使用 `CGRectMakeWithDictionaryRepresentation` 解析 `kCGWindowBounds`
  - 返回结果按前到后排序（`CGWindowListCopyWindowInfo` 默认顺序）
  - 坐标为 macOS 屏幕逻辑坐标（points），与覆盖层 CSS 坐标系一致
- 非 macOS：返回空 `Vec`（后续可用 Windows `EnumWindows` 扩展）
- 这是阻塞操作，调用方通过 `tokio::task::spawn_blocking` 包装
- **必须在覆盖层窗口创建前调用**，否则会包含覆盖层自身

**`capture_full() -> anyhow::Result<String>`**
- macOS：通过 `CGWindowListCreateImage` FFI 直接截图
- 非 macOS：通过 `xcap::Monitor` 获取主显示器并执行 `capture_image()`
- 返回 base64 编码的 PNG
- 这是阻塞操作，调用方（commands）通过 `tokio::task::spawn_blocking` 包装

**`capture_region_from_full(full_base64, x, y, width, height) -> anyhow::Result<String>`**
- 解码 base64 PNG 为内存图像
- 使用 `image::crop_imm()` 裁切指定矩形区域
- 坐标和尺寸为物理像素
- 转换为 RGBA8 后重新编码为 base64 PNG

**`image_to_base64(img: &RgbaImage) -> anyhow::Result<String>`**（内部函数）
- 使用 `Cursor<Vec<u8>>` 进行内存中 PNG 编码
- 编码为 base64 标准格式

### 数据流

```
xcap 捕获 → RgbaImage → PNG 编码 → base64 字符串
                          ↑
base64 输入 → 解码 → DynamicImage → crop_imm → RgbaImage → PNG → base64

CGWindowListCopyWindowInfo → 过滤/解析 → Vec<WindowRect> → JSON → 前端
```

## 依赖关系

- **外部依赖**：`image`（图像处理）、`base64`（编码）、`core-foundation`（macOS CF 类型）、`xcap`（非 macOS 截图）
- **系统框架**：`CoreGraphics.framework`（macOS 截图 + 窗口列表）
- **被依赖**：`commands/screenshot.rs` 调用 `capture_full()`、`capture_region_from_full()`、`list_window_rects()`

## 修改指南

- `capture_full()` 和 `list_window_rects()` 都是**阻塞调用**，必须通过 `spawn_blocking` 在异步上下文中调用
- xcap 返回的截图使用**物理像素**坐标，与前端逻辑像素不同
- `list_window_rects()` 返回的是**逻辑坐标**（points），与前端 CSS 坐标一致
- 当前仅使用第一个显示器，多显示器支持需修改 monitor 选择逻辑
- base64 编解码使用 `base64::engine::general_purpose::STANDARD`，不带 URL safe
- 图像格式固定为 PNG，如需更改需同步修改 OCR 模块对图像格式的验证
- 窗口列表数据通过 `AppState.frozen_window_rects`（`serde_json::Value`）传递，避免 config 模块对 screenshot 模块的类型依赖

## macOS 屏幕录制权限

截图功能依赖 macOS 屏幕录制权限。该权限绑定到应用的代码签名，使用 ad-hoc 签名会导致每次编译后权限失效。解决方案见 [architecture.md - macOS 代码签名](../architecture.md#macos-代码签名开发环境)。
