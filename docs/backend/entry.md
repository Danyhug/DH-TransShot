# 入口模块（lib.rs + main.rs）

## 概述

应用程序入口，负责初始化 Tauri Builder、注册插件、管理全局状态、挂载 Tauri 命令。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/lib.rs` | Tauri Builder 配置入口，注册插件、状态、命令，执行 setup |
| `src-tauri/src/main.rs` | 二进制入口，调用 `lib::run()`，Windows release 隐藏控制台 |

## 核心逻辑

### lib.rs - `run()` 函数

```rust
pub fn run() {
    let app_state = AppState::default();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![...])
        .setup(|app| {
            tray::setup_tray(app)?;
            hotkey::setup_hotkeys(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

**初始化顺序：**
1. 创建 `AppState`（默认配置 + 空冻结截图）
2. 注册插件：`tauri_plugin_global_shortcut`、`tauri_plugin_store`
3. 注册全局状态：`app_state`
4. 注册 Tauri 命令（截图 3 个 + OCR 1 个 + 翻译 1 个 + 设置 2 个 + 剪贴板 2 个）
5. setup 阶段：加载持久化配置 → 初始化系统托盘 → 注册全局快捷键

**注册的命令：**
- `start_region_select`、`capture_region`、`get_frozen_screenshot`
- `recognize_text`
- `translate_text`
- `get_settings`、`save_settings`
- `read_clipboard`、`copy_image_to_clipboard`

### main.rs

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
fn main() { dh_transshot_lib::run() }
```

- `windows_subsystem = "windows"` 在 release 模式下隐藏 Windows 控制台窗口

## 依赖关系

- **依赖**：`config::AppState`、`tray`、`hotkey`、`commands::*`
- **被依赖**：`main.rs` 调用 `lib::run()`

## 修改指南

- 新增 Tauri 命令时：在 `commands/` 下实现后，在 `invoke_handler` 的 `generate_handler!` 宏中注册
- 新增插件时：在 `.plugin()` 链中添加
- 新增全局状态字段时：修改 `config/settings.rs` 中的 `AppState`
- setup 阶段的初始化顺序可能影响功能可用性（如快捷键依赖窗口存在）
