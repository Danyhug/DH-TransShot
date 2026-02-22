# 全局快捷键模块（hotkey.rs）

## 概述

注册系统全局键盘快捷键，将按键事件转化为 Tauri 事件发送到前端。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/hotkey.rs` | 快捷键注册和事件发射 |

## 核心逻辑

### `setup_hotkeys(app) -> Result<()>`

**注册的快捷键：**

| 快捷键 | Shortcut 定义 | 发射事件 |
|--------|--------------|---------|
| `Alt+A` (macOS: `⌥A`) | `Modifiers::ALT` + `Code::KeyA` | `"hotkey-action"` → `"screenshot"` |
| `Alt+S` (macOS: `⌥S`) | `Modifiers::ALT` + `Code::KeyS` | `"hotkey-action"` → `"ocr_translate"` |

**实现：**
- 使用 `tauri_plugin_global_shortcut` 插件的 `on_shortcuts` 方法批量注册
- 回调中通过引用比较确定触发的快捷键，emit 对应 payload
- `Modifiers::ALT` 在 macOS 为 Option (⌥)，在 Windows 为 Alt

## 依赖关系

- **依赖**：`tauri::Emitter`、`tauri_plugin_global_shortcut`（`Code`、`GlobalShortcutExt`、`Modifiers`、`Shortcut`）
- **被依赖**：`lib.rs` 的 `setup` 阶段调用 `setup_hotkeys(app)`
- **事件消费者**：前端 `App.tsx` 监听 `"hotkey-action"` 事件

## 修改指南

- 修改快捷键组合需同步更新 `config/settings.rs` 中的默认值（当前快捷键是硬编码的，settings 中的值仅用于 UI 展示）
- 若需支持用户自定义快捷键，需在运行时读取 settings 并动态注册/注销快捷键
- 快捷键冲突（与系统或其他应用）可能导致注册失败，当前仅通过 `?` 向上传播错误
- 新增快捷键需创建 `Shortcut` 实例并加入 `on_shortcuts` 的数组，同时添加回调分支
