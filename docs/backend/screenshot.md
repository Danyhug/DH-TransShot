# 截图模块（screenshot/）

## 概述

封装 xcap crate 实现屏幕截图功能，提供全屏捕获和区域裁切两个核心函数，输出 base64 编码的 PNG 图片。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/screenshot/mod.rs` | 模块声明，公开导出 `capture_full` 和 `capture_region_from_full` |
| `src-tauri/src/screenshot/capture.rs` | 截图逻辑实现 |

## 核心逻辑

### capture.rs

**`capture_full() -> anyhow::Result<String>`**
- 通过 `xcap::Monitor::all()` 获取所有显示器
- 取第一个显示器（主显示器）执行 `capture_image()`
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
```

## 依赖关系

- **外部依赖**：`xcap`（截图）、`image`（图像处理）、`base64`（编码）
- **被依赖**：`commands/screenshot.rs` 调用 `capture_full()` 和 `capture_region_from_full()`

## 修改指南

- `capture_full()` 是**阻塞调用**（xcap 使用系统 API），必须通过 `spawn_blocking` 在异步上下文中调用
- xcap 返回的截图使用**物理像素**坐标，与前端逻辑像素不同
- 当前仅使用第一个显示器，多显示器支持需修改 monitor 选择逻辑
- base64 编解码使用 `base64::engine::general_purpose::STANDARD`，不带 URL safe
- 图像格式固定为 PNG，如需更改需同步修改 OCR 模块对图像格式的验证

## macOS 屏幕录制权限

xcap 截图依赖 macOS 屏幕录制权限。该权限绑定到应用的代码签名，使用 ad-hoc 签名会导致每次编译后权限失效。解决方案见 [architecture.md - macOS 代码签名](../architecture.md#macos-代码签名开发环境)。
