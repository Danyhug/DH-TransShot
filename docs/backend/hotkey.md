# 全局快捷键模块（hotkey.rs）

## 概述

使用 `tauri-plugin-global-shortcut` 注册三个可配置的系统全局快捷键，并把触发结果通过 `hotkey-action` 事件发送给前端。

## 状态

```rust
static HOTKEYS_SUSPENDED: AtomicBool = AtomicBool::new(false);
static HOTKEY_OPERATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
```

- `HOTKEYS_SUSPENDED`：设置窗口录入组合键期间为 `true`，阻止快捷键被重新注册
- `HOTKEY_OPERATION_LOCK`：串行化挂起、恢复和重载，避免设置窗口快速开关时异步命令乱序

快捷键动作直接保存在各自 `on_shortcut` handler 的闭包中，不再维护额外的全局快捷键映射或第二套 macOS 键盘监听。

## 注册

### `parse_shortcut`

使用 `Shortcut::from_str` 解析 `Alt+A`、`Ctrl+Shift+S`、`Cmd+K`、`Alt+F1` 等字符串。空值、非法按键或重复组合会记录警告并跳过。

### `apply_hotkeys`

逐个调用 `global_shortcut().on_shortcut(...)` 注册：

1. 单个组合被系统或其他应用占用时，只影响该组合
2. handler 只处理 `ShortcutState::Pressed`
3. action 名由闭包捕获，触发后交给 `dispatch_hotkey_action`

不使用批量注册，因为批量 API 会在第一项失败时中止后续注册。

### `dispatch_hotkey_action`

后台等待 Alt/Option 释放后 emit `hotkey-action`。等待修饰键释放可以避免截图窗口切换焦点或模拟复制时把仍按下的 Alt 带入后续操作。

触发动作后不会自动注销/重注册快捷键。频繁刷新系统注册既没有必要，也会增加注销失败后出现“已占用”残留状态的概率。

## 设置窗口录入

已经注册的系统快捷键不会到达浏览器 `keydown`，因此设置窗口打开时需要暂时注销：

### `suspend_hotkeys`

1. 获取 `HOTKEY_OPERATION_LOCK`
2. 确认 `settings` 窗口仍然存在，忽略窗口销毁后晚到的异步请求
3. 原子地设置 suspended 状态；重复调用直接返回
4. 调用 `unregister_all`
5. 注销完成后再次检查窗口；若窗口期间已关闭，立即恢复

### `resume_hotkeys` / `restore_hotkeys`

只有第一个把 suspended 从 `true` 切换回 `false` 的调用者会执行注册。以下三条恢复路径可以安全重叠：

- 保存/取消按钮关闭窗口前显式调用 `resume_hotkeys`
- React effect cleanup
- Rust 监听 `settings` 窗口 `Destroyed` 事件后调用 `restore_hotkeys`

原生 `Destroyed` 监听是必要兜底：Tauri 销毁 webview 时不保证 React cleanup 执行。缺少该兜底会让进程永久停在挂起状态，macOS 上表现为 `Option+S` 直接输入 `ß`。

## 重载

`reload_hotkeys` 只在配置保存或设置录入结束时调用：

1. 获取操作锁
2. suspended 时跳过
3. 注销现有快捷键
4. 从 `AppState.settings.hotkeys` 读取最新配置
5. 重新逐项注册

## 动作

| action | 默认组合 | 功能 |
|---|---|---|
| `screenshot` | `Alt+A` | 区域截图 |
| `ocr_translate` | `Alt+S` | 区域翻译 |
| `clipboard_translate` | `Alt+Q` | 翻译选中文本 |

前端 `App.tsx` 监听 `hotkey-action` 并分派对应动作。

## 维护约束

- 不要在 handler 内执行 IO 或窗口操作；保持快速返回
- 保持 `Pressed` 触发，修饰键释放由 `dispatch_hotkey_action` 处理
- 不要在每次触发或截图覆盖层关闭后刷新注册
- 新增动作时同步修改 `HotkeyConfig`、`apply_hotkeys`、前端类型和设置界面
