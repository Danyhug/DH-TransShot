# 截图模块（screenshot/）

## 概述

封装屏幕截图功能，提供逐显示器捕获、区域裁切和窗口矩形列表采集，输出 base64 编码的 PNG 图片。
macOS 的抓屏走 **ScreenCaptureKit**（失败时回退 Core Graphics），窗口矩形列表仍走 `CGWindowListCopyWindowInfo`。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/screenshot/mod.rs` | 模块声明，公开导出 `capture_monitors`、`capture_region_from_full`、`capture_region_bytes`、`list_window_rects`，以及覆盖层 label 前缀常量 `OVERLAY_LABEL_PREFIX` |
| `src-tauri/src/screenshot/capture.rs` | 截图逻辑实现 + 窗口矩形列表采集 |
| `src-tauri/src/screenshot/sck.rs` | macOS 抓屏：ScreenCaptureKit 单帧截图（objc2 动态调用），含屏幕录制权限预检 |

## 为什么 macOS 用 ScreenCaptureKit

这里踩过一个很典型的坑：**系统弹窗、部分页面截不上，而且看起来像是「覆盖层层级不够」**（v1.1.7 之前的实现）。
实际原因在抓屏 API 本身——老的 `CGWindowListCreateImage` 合成的不是屏幕像素，而是「**允许共享的窗口**」：

- SDK 的 `CGWindowSharingType` 说明：`kCGWindowSharingNone` 的窗口**不参与合成**
- 系统弹窗 / 授权框 / 登录面板基本都属于 macOS 刻意保护的 UI
- 硬件解码视频、Metal / IOSurface 图层没有落在窗口后备存储里，合成结果是黑的
- 只覆盖**当前 Space**，别的 Space / 全屏 App 里的窗口进不来

覆盖层显示的是**建窗之前**拍好的冻结图（见 [architecture.md](../architecture.md) 的事件流），
所以把覆盖层提到多高的 window level 都补不回缺失的像素——缺的是那张图本身。

ScreenCaptureKit 拿的是 WindowServer **合成后的显示帧缓冲**，屏幕上看得见的内容它都能拿到；
Apple 从 macOS 15 起把 `CGWindowListCreateImage` 标成 obsoleted（SDK：
`SCREEN_CAPTURE_OBSOLETE(10.5, 14.0, 15.0)`）并明确要求迁到 SCK。

仍然拿不到的只剩 macOS 自己过滤掉的安全 UI（登录窗口、TCC 授权框、安全输入框）。这是系统级限制，
没有公开 API 能绕过，不要试图用私有 API 硬撬。

### 实现要点

- 运行时按类名查找（`SCShareableContent` / `SCScreenshotManager`），找不到就静默回退老的 CG 路径，
  所以最低系统版本不受影响（单帧截图 `SCScreenshotManager` 需要 macOS 14+）
- `SCDisplay` 和 Tauri 的显示器顺序不保证一致，用 `CGGetActiveDisplayList` + `CGDisplayBounds`
  反查 `CGDirectDisplayID`，再按 `displayID` 找对应的 `SCDisplay`
- 抓的是**整块显示器**，尺寸就是原生像素尺寸，和原来 CG 的结果一致，下游裁切逻辑不用改
- `SCStreamConfiguration.showsCursor = false`，与 `CGWindowListCreateImage` 的行为对齐
- CGImage 在回调返回后会被释放，所以回调里先 `CFRetain` 再带出去，由调用方 `CFRelease`
- 回调里的 Objective-C 对象（`SCShareableContent` / `SCDisplay` / `SCContentFilter`）**不要跨线程传递**，
  整条链都在回调内完成
- 截图前做一次 `CGPreflightScreenCaptureAccess()` 预检：缺权限时老 CG 路径会**静默只返回桌面**，
  现在会 `CGRequestScreenCaptureAccess()` 请求授权并打一条 warn
- `SCShareableContent` 首次查询要和 WindowServer 握手（200~300ms），启动时在后台 `sck::warm_up()` 预热。
  **预热只在已有屏幕录制权限时执行**：没权限就去查会当场弹授权框，会打扰不用截图的用户
- 过滤器与配置对象在回调里 `CFRelease`：它们要活到截图完成之后，在调用处立即释放是悬垂引用

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
- 与 `capture_monitors()` 互不依赖（两者都只读 WindowServer），调用方应让它们并行执行

**`capture_monitors(monitors: &[(f64, f64, f64, f64)]) -> anyhow::Result<Vec<String>>`**
- 逐显示器捕获截图，每个显示器返回一张独立的 base64 PNG
- 参数为每个显示器的逻辑坐标 `(x, y, width, height)`
- macOS：优先 `sck::capture_display()`（ScreenCaptureKit 单帧截图），失败才回退 `CGWindowListCreateImage`
  - SCK 拿的是显示器合成后的帧缓冲，系统弹窗 / 硬件合成内容都能进来，见上文「为什么 macOS 用 ScreenCaptureKit」
  - 回退路径用每个显示器的逻辑矩形 + `CGWindowListCreateImage(rect, kCGWindowListOptionOnScreenOnly, …)`
  - 每张图像为该显示器的原生分辨率（2x Retina 显示器返回 2x 图像）
  - 避免了混合 DPI 时单张合成图像的非均匀缩放问题
  - 多显示器并行截图（`std::thread::spawn`），缩短总截图耗时
  - 每个显示器会分别记录抓屏耗时和「转 RGBA + PNG 编码」耗时，用于区分系统截图与本地编码各占多少
- 非 macOS：通过 `xcap::Monitor::all()` 逐个捕获
- 这是阻塞操作，调用方通过 `tokio::task::spawn_blocking` 包装

**`cgimage_to_base64(cg_image) -> anyhow::Result<String>`**（macOS 内部函数）
- 将 CGImage 转换为 base64 PNG
- CG 返回 BGRA 格式，转换为 RGBA
- 按行处理，正确处理 `bytes_per_row` 对齐

**`capture_region_from_full(full_base64, x, y, width, height) -> anyhow::Result<String>`**
- 解码 base64 PNG 为内存图像
- 使用 `image::crop_imm()` 裁切指定矩形区域
- 坐标和尺寸为图像像素坐标（对应该显示器的原生分辨率）
- 编码为 JPEG 后返回 base64（比 PNG 更小更快，截图无透明通道）
- 调用方必须通过 `tokio::task::spawn_blocking` 包装，避免阻塞 async 运行时

**`capture_region_bytes(full_base64, x, y, width, height) -> anyhow::Result<Vec<u8>>`**
- 与 `capture_region_from_full` 相同的裁切逻辑，但返回原始 JPEG 字节（不做 base64 编码）
- 用于 `capture_and_ocr` 命令，避免 base64 编码→解码的往返开销
- 调用方必须通过 `tokio::task::spawn_blocking` 包装

**`image_to_base64(img: &RgbaImage) -> anyhow::Result<String>`**（内部函数）
- 内存中 PNG 编码：`PngEncoder::new_with_quality(Fast, Up)`（见下文「修改指南」，别用默认档）
- 编码为 base64 标准格式

**`image_to_jpeg_base64(img: &DynamicImage) -> anyhow::Result<String>`**（内部函数）
- 转换为 RGB8 后使用 JPEG 编码器（quality=90）编码
- 编码为 base64 标准格式

### 数据流

```
capture_monitors() → 逐显示器并行截图 → Vec<base64 PNG>
                                          ↓
                     每个覆盖层窗口获取自己对应显示器的图像
                                          ↓
       base64 PNG → 解码 → DynamicImage → crop_imm → RGB8 → JPEG → base64

       capture_region_bytes() → 同上裁切，但返回原始 JPEG 字节（供 capture_and_ocr 使用）

CGWindowListCopyWindowInfo → 过滤/解析 → Vec<WindowRect> → JSON → 前端
```

## 依赖关系

- **外部依赖**：`image`（图像处理）、`base64`（编码）、`objc2` + `block2`（macOS 调 SCK）、`xcap`（非 macOS 截图）
- **系统框架**：`ScreenCaptureKit`（macOS 抓屏，运行时按类名查找）、`CoreGraphics.framework`（窗口列表 / 显示器枚举 / CG 回退路径）、`CoreFoundation`（CFRetain / CFRelease）
- **被依赖**：`commands/screenshot.rs` 调用 `capture_monitors()`、`capture_region_from_full()`、`list_window_rects()`

## 覆盖层窗口的创建与销毁

覆盖层窗口（label `screenshot-overlay-<显示器序号>`）不在本模块创建/销毁，而在 `commands/screenshot.rs`；label 前缀统一用本模块的 `OVERLAY_LABEL_PREFIX`，不要散落字面量。

**销毁必须走 `window_lifecycle`**（先 `hide()` 让帧、再 `close()`）。直接 `close()` 一个还活着的 webview 会让 WebKit 的 display link 偶发访问已释放的滚动树，把整个 App 打崩（`EXC_BAD_ACCESS (SIGSEGV)`，崩溃栈上没有本项目的帧）。崩溃现场、成因和修改指南见 [window_lifecycle.md](window_lifecycle.md)。

## 修改指南

- `capture_monitors()`、`list_window_rects()` 和 `capture_region_from_full()` 都是**阻塞调用**，必须通过 `spawn_blocking` 在异步上下文中调用
- macOS 抓屏不要退回只用 `CGWindowListCreateImage`：它合成的是「允许共享的窗口」，系统弹窗与硬件合成内容会缺，
  而且是 macOS 15 起就 obsoleted 的 API。要改就改 `sck.rs`，但保持 CG 回退路径可用
- 判断 SCK 是否可用统一用 `sck::is_available()`（按类名查找），不要写死系统版本号
- 每个显示器的截图为该显示器的**原生分辨率**（如 2x Retina 返回 2× 物理像素图像）
- `list_window_rects()` 返回的是**逻辑坐标**（points），与前端 CSS 坐标一致
- `capture_monitors()` 传入的坐标为逻辑坐标（`Tauri PhysicalPosition / scale_factor`）
- base64 编解码使用 `base64::engine::general_purpose::STANDARD`，不带 URL safe
- 区域裁切输出为 JPEG（`capture_region_from_full` 和 `capture_region_bytes`），整屏捕获输出为 PNG（`capture_monitors`）
- 整屏 PNG 用 `PngEncoder` 的 `Fast + Up`，**不要改回 `write_to(.., ImageFormat::Png)` 的默认档**：默认档按行试完 5 种滤波再走 deflate，内容偏照片/视频时 2560×1664 要 610ms（Fast+Up 约 24ms），而它正好卡在覆盖层出现的关键路径上
- 窗口列表数据通过 `AppState.frozen_window_rects`（`serde_json::Value`）传递，避免 config 模块对 screenshot 模块的类型依赖

## macOS 屏幕录制权限

截图功能依赖 macOS 屏幕录制权限。该权限绑定到应用的代码签名，使用 ad-hoc 签名会导致每次编译后权限失效。解决方案见 [architecture.md - macOS 代码签名](../architecture.md#macos-代码签名开发环境)。

权限缺失时**不会报错**，而是表现为「截图只有桌面壁纸」——`CGWindowListCreateImage` 与 ScreenCaptureKit
都会返回一张内容几乎为空的有效图像。所以 `capture_monitors_macos()` 在抓屏前会做一次
`CGPreflightScreenCaptureAccess()` 预检，缺权限时调 `CGRequestScreenCaptureAccess()` 请求授权，
并在日志里打一条 warn（前缀 `[Capture]`）。排查「截图缺内容」时先看这条日志。
