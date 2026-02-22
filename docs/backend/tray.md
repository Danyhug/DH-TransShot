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
| `show` | Show Window | 显示并聚焦主窗口 |
| `screenshot` | 区域截图 (Alt+A) | emit `"tray-action"` → `"screenshot"` |
| `ocr_translate` | 区域翻译 (Alt+S) | emit `"tray-action"` → `"ocr_translate"` |
| `sep` | ───────── | 分隔线（disabled） |
| `quit` | Quit | `app.exit(0)` 退出应用 |

**图标加载优先级：**
1. `icons/32x32.png`（文件系统路径）
2. `app.default_window_icon()`（Tauri 内置默认图标）
3. `include_bytes!("../icons/32x32.png")`（编译时嵌入的图标）

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
- 托盘图标路径 `icons/32x32.png` 为相对路径，开发和打包环境下行为可能不同
