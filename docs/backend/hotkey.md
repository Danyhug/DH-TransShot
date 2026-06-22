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
static HOTKEYS_SUSPENDED: AtomicBool = AtomicBool::new(false);
```

- `SHORTCUT_ACTIONS` — 进程级单例的快捷键 → 动作名映射表，由 `apply_hotkeys` 写入，由 `handle_shortcut_event` 读取。`Shortcut` 实现 `Hash + Eq + Copy`，可直接作为 HashMap 键
- `HOTKEYS_SUSPENDED` — `reload_hotkeys` 的"挂起"门禁。`suspend_hotkeys` 置 true、`resume_hotkeys` 清 false。`reload_hotkeys` 在 true 时直接 no-op，防止 `emit_hotkey_action` 或覆盖层关闭的延迟重注册在用户录入新快捷键时把旧快捷键又装回去
- `LAST_HOTKEY_DISPATCH` — macOS Carbon 和 `CGEventTap` 兜底同时观察到同一次按键时的短窗口去重，避免同一动作被发两次

### `handle_shortcut_event(&AppHandle, &Shortcut, ShortcutEvent)`

注册为 per-shortcut handler（在 `apply_hotkeys` 中通过 `on_shortcuts(shortcuts, handle_shortcut_event)` 装载）。

- 只在 `ShortcutState::Pressed` 触发。**为什么不是 Released？** 在 macOS 上，当快捷键动作会抢占键盘焦点（例如打开截图覆盖层）时，用户后续松开修饰键（Alt/Option）的事件可能被送到新的前台应用，而不是 Carbon `RegisterEventHotKey` 系统。一旦 Carbon 漏收一次 Release，它会认为快捷键仍处于按下状态，**之后所有的相同按键都收不到 Pressed 事件**（典型症状：首次正常、后续失灵）。改用 Pressed 即可绕过这个状态机问题
- 在 `SHORTCUT_ACTIONS` 中查找对应动作名，找到则交给 `dispatch_hotkey_action` 去重后异步发送
- 修饰键仍然按下的问题由 `emit_hotkey_action` 内部主动轮询处理，不依赖事件传递

### macOS `CGEventTap` 兜底

`macos_event_tap` 在 macOS 上额外安装一个 `CGEventTap`，监听 `KeyDown` 并按当前 `SHORTCUT_ACTIONS` 同步出的 scancode + modifier flags 匹配快捷键。匹配成功时：

1. 非自动重复按键会调用 `dispatch_hotkey_action`
2. 返回 `null` 吞掉该按键，避免 Carbon 丢绑定时 `Option+S` 继续透传给前台输入框并输入 `ß`
3. 设置面板录入快捷键期间，`HOTKEYS_SUSPENDED` 为 true，event tap 直接放行按键

**重要：CGEventTap 需要辅助功能权限才能工作。** 如果 `CGEventTapCreate` 返回 null（缺少辅助功能权限），应用会：
1. 调用 `AXIsProcessTrusted()` 检查权限状态
2. 打印详细的 warn 日志，提示用户在 系统设置 > 隐私与安全性 > 辅助功能 中授权 DH-TransShot
3. 继续使用 Carbon 快捷键路径（但无法吞掉事件，按 Option+Q 等组合键仍会输入特殊字符）

`Info.plist` 中已声明 `NSAccessibilityUsageDescription`，系统会在首次请求辅助功能权限时显示说明。

### `emit_hotkey_action(AppHandle, String)`

后台线程中:

1. 等待 Alt/Option 真正释放(macOS 用 `CGEventSourceFlagsState(1)` 轮询;Windows/Linux 用 120ms 固定 sleep)
2. `emit("hotkey-action", action)` 通知前端
3. **再 sleep 2s,然后调用 `reload_hotkeys`** —— 主动刷新 Carbon hotkey 注册,防止动作中创建的窗口、osascript、CGEvent 等 OS 操作把绑定弄丢。2s 足够覆盖大多数动作的 OS 阶段;`reload_hotkeys` 在 suspended 状态下会跳过,所以不会干扰设置面板录入

为什么 Step 3 必要:macOS Carbon `RegisterEventHotKey` 在我们的覆盖层/AppleScript 操作后会偶发"丢绑定" —— plugin 内部 HashMap 还认为快捷键已注册,但 OS 层已经把绑定弄丢了,按键直接透传到当前焦点应用(典型症状:在文本框里按 Alt+S 输入了 ß 而不是触发截图)。每次触发后主动重注册一次能 mask 这个问题。

### `parse_shortcut(&str) -> Result<Shortcut, String>`

直接复用 `tauri_plugin_global_shortcut::Shortcut::from_str`（底层是 `global_hotkey::HotKey::from_str`）。

**支持格式：** `Alt+A`、`Ctrl+Shift+S`、`Cmd+K`、`Alt+F1`、`Ctrl+Space`、`Cmd+,` 等。修饰键支持 `Alt`/`Option`/`Ctrl`/`Control`/`Shift`/`Cmd`/`Command`/`Super`/`CmdOrCtrl`，主键支持 A-Z、0-9、F1-F24、Space、Enter、Tab、Escape、方向键、常见标点等。

### `apply_hotkeys(&AppHandle, &HotkeyConfig)`

- 遍历 `screenshot` / `ocr_translate` / `clipboard_translate` 三个动作
- 解析失败 / 与同一批次内其他快捷键冲突 → 打 `warn` 日志并跳过该项（其他仍正常注册，避免一项错配置导致用户无快捷键可用）
- 用新映射整体替换 `SHORTCUT_ACTIONS`，然后调用 `global_shortcut().on_shortcuts(shortcuts, handle_shortcut_event)` 批量注册

**为什么用 `on_shortcuts` 而不是 `register_multiple` + `with_handler`？** plugin 内部两条路径都走同一个 `shortcuts_.lock().get(&e.id)` 查表，理论上等价；但实测在 macOS 上 v2.3.1 的 global handler 路径偶尔会丢事件（提交 d8a9e23 切到 global handler 后引入的回归），per-shortcut handler 路径更稳定。

### `setup_hotkeys(&tauri::App) -> Result<()>`

应用启动时调用一次（`lib.rs::setup()`）。从 `AppState.settings.hotkeys` 读取当前配置并调用 `apply_hotkeys`。

### `reload_hotkeys(&AppHandle)`

读取 `AppState.settings.hotkeys` 后重新调用 `apply_hotkeys`。**`HOTKEYS_SUSPENDED` 为 true 时直接 no-op**。

调用时机:

1. `commands/settings.rs::save_settings()` 末尾 —— 设置保存后立即生效(但若此时 suspended,实际生效由 `resume_hotkeys` 承担)
2. `commands/screenshot.rs` 覆盖层 `WindowEvent::Destroyed` —— 覆盖层关闭后的 Carbon 状态刷新(详见前述"Carbon 丢绑定"问题)
3. `emit_hotkey_action` 末尾 —— 每个快捷键触发完成 2s 后自动刷新

注意第 2、3 两处必须在**后台线程**调用(`std::thread::spawn`),因为 `reload_hotkeys` 内部通过 `run_on_main_thread` 派任务,而 `on_window_event` 等回调可能本身已在主线程,直接调会死锁。

### `suspend_hotkeys` / `resume_hotkeys`(Tauri 命令)

为 SettingsPanel 录入新快捷键服务,并保证延迟重注册不会"撤回"用户的新设置。

**问题背景:** 当某个组合(如 `Alt+Q`)已被注册为全局快捷键时,操作系统会在按键到达浏览器前把事件拦截给全局 handler,导致前端 `HotkeyInput` 的 `keydown` 监听器收不到事件,UI 一直停留在"按下组合键..."状态。

**方案:**

- `suspend_hotkeys` — 置 `HOTKEYS_SUSPENDED=true`,然后 `global_shortcut().unregister_all()`。此后所有 `reload_hotkeys` 调用都会跳过,直到 resume(这点很重要 —— `emit_hotkey_action` 在动作触发 2s 后会调 reload,如果不被门禁拦住,会把用户正在录入的 UI 状态毁掉)
- `resume_hotkeys` — 清 `HOTKEYS_SUSPENDED=false`,再调用 `reload_hotkeys`(此时已从最新 settings 读取新组合)

前端 `SettingsPanel` 在 mount 时调用 `suspend_hotkeys`,在 unmount 时调用 `resume_hotkeys`。配合 `save_settings` 自身在末尾也会触发 reload(suspended 状态下是 no-op,但 unmount 时的 resume 会重做),从而做到:保存 → 新快捷键 unmount 时立即生效;取消 → 仍恢复旧快捷键。

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
  - `lib.rs::run()` — 构建 plugin 时**不再使用** `with_handler`；handler 改在 `apply_hotkeys` 中通过 `on_shortcuts` 按 shortcut 装载；`setup()` 调用 `setup_hotkeys`
  - `commands/settings.rs::save_settings` — 保存后调用 `reload_hotkeys` 立即生效
- **事件消费者**：前端 `App.tsx` 监听 `"hotkey-action"` 事件

## 修改指南

- 新增快捷键动作：在 `HotkeyConfig` 中加字段 → 在 `apply_hotkeys` 的 `entries` 数组追加条目 → 前端 `types/index.ts` 和 `SettingsPanel` 同步
- 快捷键冲突（与系统或其他应用）会导致 `on_shortcuts` 返回 Err，目前只打 `warn` 日志，不阻断应用
- `SHORTCUT_ACTIONS` 写入与 `on_shortcuts` 之间存在极短窗口期，事件回调中找不到 mapping 时静默忽略，不会 panic
- 若需要在快捷键解析失败时阻断保存，前端 `SettingsPanel.save` 已做空值校验，可扩展为字符串格式校验
- 不要在 `handle_shortcut_event` 内做耗时操作（如 IO / 锁等待），避免阻塞全局键盘事件循环
- **不要把 handler 改回监听 `Released`**：这会重新引入"首次正常、后续失灵"的问题（详见 `handle_shortcut_event` 注释）。修饰键残留问题应通过 `emit_hotkey_action` 内部主动轮询解决，而不是依赖 OS 事件传递
