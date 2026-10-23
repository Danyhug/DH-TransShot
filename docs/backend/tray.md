# 系统托盘模块（tray.rs）

## 概述

创建系统托盘图标和菜单，将用户操作路由为 Tauri 事件。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/tray.rs` | 系统托盘初始化、菜单构建、事件处理 |

## 核心逻辑

### `setup_tray(app) -> Result<()>`

**菜单项：**

| ID | 标签 | 功能 |
|----|------|------|
| `version` | DH-TransShot v{版本号} | 显示当前版本号（disabled，仅展示）；版本号取自 `app.package_info().version`（由 `tauri.conf.json` 同步） |
| `sep_version` | ───────── | 分隔线（disabled） |
| `show` | Show Window | 显示并聚焦主窗口 |
| `screenshot` | 区域截图 (当前设置快捷键) | emit `"tray-action"` → `"screenshot"` |
| `ocr_translate` | 区域翻译 (当前设置快捷键) | emit `"tray-action"` → `"ocr_translate"` |
| `clipboard_translate` | 翻译选中文本 (当前设置快捷键) | emit `"tray-action"` → `"clipboard_translate"` |
| `sep` | ───────── | 分隔线（disabled） |
| `quit` | Quit | `app.exit(0)` 退出应用 |

**图标：**

图标用 `include_bytes!` 在编译期嵌入二进制，不再读运行时路径。以前首选 `Image::from_path("icons/32x32.png")`，但打包成 `.app` 后进程的工作目录是 `/`，相对路径读不到，只能落到 `default_window_icon()` 或 `include_bytes!` 兜底 —— 开发时和打包后拿到的可能不是同一张图。

| 平台 | 文件 | 说明 |
|------|------|------|
| macOS | `icons/tray-macos.png` | 模板图（template image）：纯色 + alpha、无底板，配合 `icon_as_template(true)` 由系统按菜单栏明暗自动反色 |
| Windows / Linux | `icons/tray-windows.png` | 没有模板图机制，保留 App 图标的深色圆角底板，保证亮/暗任务栏都看得清 |

两个文件的源 SVG 是同目录下的 `tray-macos.svg` / `tray-windows.svg`，用 `pnpm tauri icon <svg> -o <tmp> -p 256` 栅格化成 256×256 PNG 后复制过来。

**为什么不用 App 图标 `32x32.png`：** macOS 的 `tray-icon` crate 会把托盘图强制缩放到 **18pt 高**（保持宽高比）。App 图标受 macOS 图标网格约束，图形只占画布的 50%（1024 画布 / 824 圆角方块 / 520 图形），进菜单栏后图形只剩约 **10pt**，实机就是「托盘图标太小」。专用托盘图把图形铺到画布的 88%，同样 18pt 下图形约 **16pt**，与系统图标（Wi‑Fi、控制中心）一致。

**行为：**
- 左键点击托盘图标直接显示菜单（`show_menu_on_left_click(true)`）
- `show` 操作通过 `app.get_webview_window("main")` 获取主窗口并调用 `show()` + `set_focus()`

## 依赖关系

- **依赖**：`tauri::menu`、`tauri::tray`、`tauri::Emitter`、`tauri::Manager`
- **被依赖**：`lib.rs` 的 `setup` 阶段调用 `setup_tray(app)`
- **事件消费者**：前端 `App.tsx` 监听 `"tray-action"` 事件

## 修改指南

- 新增菜单项需使用 `MenuItem::with_id()` 创建，并在 `Menu::with_items()` 中注册
- 菜单事件通过 `on_menu_event` 闭包处理，新增 action 需在 match 中添加分支
- 前端需同步监听新增的 action payload
- 托盘图标一律用 `include_bytes!` 嵌入，**不要改回运行时读文件路径**（打包后工作目录是 `/`）
- 改托盘图案要同步更新 `icons/tray-macos.svg` / `tray-windows.svg` 和对应的 PNG；托盘图**不要直接复用 App 图标**，理由见上
- macOS 的托盘图必须是模板图（纯色 + alpha），`icon_as_template(true)` 不能去掉，否则深色菜单栏下会看不清
