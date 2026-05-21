# 全局快捷键模块（hotkey.rs）

## 概述

注册系统全局键盘快捷键，将按键事件转化为 Tauri 事件发送到前端。**支持运行时用户自定义快捷键，保存设置后立即生效**。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/hotkey.rs` | 快捷键解析、注册、动态重载与事件分发 |

## 核心逻辑

### 全局状态

```rust
static SHORTCUT_ACTIONS: OnceLock<Mutex<HashMap<Shortcut, String>>> = OnceLock::new();
```

`SHORTCUT_ACTIONS` 是进程级单例的快捷键 → 动作名映射表，由 `apply_hotkeys` 写入，由 `handle_shortcut_event` 读取。`Shortcut` 实现 `Hash + Eq + Copy`，可直接作为 HashMap 键。

### `handle_shortcut_event(&AppHandle, &Shortcut, ShortcutEvent)`

注册为 plugin builder 级别的全局 handler（在 `lib.rs` 中通过 `Builder::with_handler` 装载）。

- 只在 `ShortcutState::Pressed` 触发。**为什么不是 Released？** 在 macOS 上，当快捷键动作会抢占键盘焦点（例如打开截图覆盖层）时，用户后续松开修饰键（Alt/Option）的事件可能被送到新的前台应用，而不是 Carbon `RegisterEventHotKey` 系统。一旦 Carbon 漏收一次 Release，它会认为快捷键仍处于按下状态，**之后所有的相同按键都收不到 Pressed 事件**（典型症状：首次正常、后续失灵）。改用 Pressed 即可绕过这个状态机问题
- 在 `SHORTCUT_ACTIONS` 中查找对应动作名，找到则交给 `emit_hotkey_action` 异步发送
- 修饰键仍然按下的问题由 `emit_hotkey_action` 内部主动轮询处理，不依赖事件传递

### `emit_hotkey_action(AppHandle, String)`

后台线程中等待 Alt/Option 真正释放后再 `emit("hotkey-action", action)`：

- **macOS**：调用 `CGEventSourceFlagsState(1)` 轮询底层修饰键状态，最长等待 500ms（20 × 25ms）。这样保证后续创建覆盖层、模拟按键等 OS 操作不会被残留的 Alt 状态污染
- **Windows/Linux**：使用固定 120ms 延迟即可（这两个平台的全局快捷键不会出现 macOS 那种 Carbon 状态卡死问题）

### `parse_shortcut(&str) -> Result<Shortcut, String>`

直接复用 `tauri_plugin_global_shortcut::Shortcut::from_str`（底层是 `global_hotkey::HotKey::from_str`）。

**支持格式：** `Alt+A`、`Ctrl+Shift+S`、`Cmd+K`、`Alt+F1`、`Ctrl+Space`、`Cmd+,` 等。修饰键支持 `Alt`/`Option`/`Ctrl`/`Control`/`Shift`/`Cmd`/`Command`/`Super`/`CmdOrCtrl`，主键支持 A-Z、0-9、F1-F24、Space、Enter、Tab、Escape、方向键、常见标点等。

### `apply_hotkeys(&AppHandle, &HotkeyConfig)`

- 遍历 `screenshot` / `ocr_translate` / `clipboard_translate` 三个动作
- 解析失败 / 与同一批次内其他快捷键冲突 → 打 `warn` 日志并跳过该项（其他仍正常注册，避免一项错配置导致用户无快捷键可用）
- 用新映射整体替换 `SHORTCUT_ACTIONS`，然后调用 `global_shortcut().register_multiple()` 批量注册

### `setup_hotkeys(&tauri::App) -> Result<()>`

应用启动时调用一次（`lib.rs::setup()`）。从 `AppState.settings.hotkeys` 读取当前配置并调用 `apply_hotkeys`。

### `reload_hotkeys(&AppHandle)`

设置保存后调用（`commands/settings.rs::save_settings()` 末尾）。

1. `global_shortcut().unregister_all()` 清理旧绑定
2. 从 `AppState.settings.hotkeys` 读取最新配置
3. 调用 `apply_hotkeys` 重新写入映射表并注册

### `suspend_hotkeys` / `resume_hotkeys`（Tauri 命令）

为 SettingsPanel 录入新快捷键服务。

**问题背景：** 当某个组合（如 `Alt+Q`）已被注册为全局快捷键时，操作系统会在按键到达浏览器前把事件拦截给全局 handler，导致前端 `HotkeyInput` 的 `keydown` 监听器收不到事件，UI 一直停留在"按下组合键..."状态。

**方案：**
- `suspend_hotkeys` — 调用 `global_shortcut().unregister_all()`，让键盘事件能被浏览器接收
- `resume_hotkeys` — 调用 `reload_hotkeys`，从最新 settings 重新注册（无论 settings 是否已保存）

前端 `SettingsPanel` 在 mount 时调用 `suspend_hotkeys`，在 unmount 时调用 `resume_hotkeys`。配合 `save_settings` 自身也会触发 `reload_hotkeys`，从而做到：保存 → 新快捷键立即生效；取消 → 仍恢复旧快捷键。

## 事件载荷

| 动作名 | 触发场景 |
|--------|---------|
| `"screenshot"` | 区域截图（默认 `Alt+A`） |
| `"ocr_translate"` | 区域翻译（默认 `Alt+S`） |
| `"clipboard_translate"` | 翻译选中文本（默认 `Alt+Q`） |

前端 `App.tsx` 监听 `"hotkey-action"` 事件，根据 payload 字符串分派。

## 依赖关系

- **依赖**：`tauri::{AppHandle, Emitter, Manager}`、`tauri_plugin_global_shortcut`、`crate::config::{AppState, HotkeyConfig}`
- **被依赖**：
  - `lib.rs::run()` — `Builder::with_handler(hotkey::handle_shortcut_event)` 装载全局 handler；`setup()` 调用 `setup_hotkeys`
  - `commands/settings.rs::save_settings` — 保存后调用 `reload_hotkeys` 立即生效
- **事件消费者**：前端 `App.tsx` 监听 `"hotkey-action"` 事件

## 修改指南

- 新增快捷键动作：在 `HotkeyConfig` 中加字段 → 在 `apply_hotkeys` 的 `entries` 数组追加条目 → 前端 `types/index.ts` 和 `SettingsPanel` 同步
- 快捷键冲突（与系统或其他应用）会导致 `register_multiple` 返回 Err，目前只打 `warn` 日志，不阻断应用
- `SHORTCUT_ACTIONS` 写入与 `register_multiple` 之间存在极短窗口期，事件回调中找不到 mapping 时静默忽略，不会 panic
- 若需要在快捷键解析失败时阻断保存，前端 `SettingsPanel.save` 已做空值校验，可扩展为字符串格式校验
- 不要在 `handle_shortcut_event` 内做耗时操作（如 IO / 锁等待），避免阻塞全局键盘事件循环
- **不要把 handler 改回监听 `Released`**：这会重新引入"首次正常、后续失灵"的问题（详见 `handle_shortcut_event` 注释）。修饰键残留问题应通过 `emit_hotkey_action` 内部主动轮询解决，而不是依赖 OS 事件传递
