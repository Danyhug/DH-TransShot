# macOS 权限说明

记录 macOS 下各功能依赖的系统权限，以及 **「翻译选中文本」（Alt+Q）在 macOS 27 上失效** 的问题、原因与修复（v1.1.2 已修复，见文末「修复」）。

> 下文「问题现象」「原因」描述的是修复前的实现，保留作为背景；引用的行号对应修复前的代码。

## 概述

DH-TransShot 在 macOS 上用到两类隐私权限：**屏幕录制**（截图/OCR）和 **辅助功能**（读取选中文字）。两者是独立的 TCC 服务，缺哪个只影响对应功能。

## 权限对照

| 功能 | 默认快捷键 | 依赖权限 | macOS ≤ 26 设置入口 | macOS 27 设置入口 |
|---|---|---|---|---|
| 区域截图 | `Alt+A` | 屏幕录制 | 隐私与安全性 → 屏幕录制 | 隐私与安全性 → 屏幕录制 |
| 区域翻译 | `Alt+S` | 屏幕录制 | 隐私与安全性 → 屏幕录制 | 隐私与安全性 → 屏幕录制 |
| 翻译选中文本 | `Alt+Q` | 辅助功能（+ 自动操作，见下） | 隐私与安全性 → 辅助功能 | 隐私与安全性 → **设备控制和数据访问** |

> macOS 27（Golden Gate）把「辅助功能 / Accessibility」从「隐私与安全性」中移除，改名为 **「设备控制和数据访问」（Device Control and Data Access）**。权限服务本身没有删除，只是设置入口和显示名变了。

## 问题现象

1. 按 `Alt+Q` **没有任何反应**：不弹窗、不报错、主窗口也不出现，只有前端日志里一条 warn。
2. 在「系统设置 → 隐私与安全性」里**找不到「辅助功能」这一项**，因此无法按旧文档去给应用授权。
3. `Alt+A` / `Alt+S` 正常——它们只依赖屏幕录制权限，不受影响。

## 原因

### 原因一：macOS 27 把「辅助功能」改名并挪了位置

macOS 27 起，Accessibility 不再是「隐私与安全性」下的独立条目，而是并入改名后的 **「设备控制和数据访问」**。

- 影响：按旧文档（README、Info.plist 等）去「辅助功能」里找入口，必然找不到。
- 注意：权限服务（TCC）没有被移除，Apple 官方企业文档在 macOS 27 中仍然使用 "accessibility permissions" 的表述。

### 原因二：应用从未主动申请过辅助功能权限

全仓库没有任何 `AXIsProcessTrusted` / `AXIsProcessTrustedWithOptions` / `AXUIElement` 调用，读取选中文字完全委托给 `osascript`。

macOS 只会把**主动申请过权限**或**用户手动添加**的应用列进设置列表。应用不申请，就不会出现在列表里——所以即使进了改名后的页面，列表里也没有 DH-TransShot。

### 原因三：Alt+Q 的两条路径都需要辅助功能权限

`Alt+Q` → `read_selected_text()`（`src-tauri/src/commands/clipboard.rs:165`），macOS 下有两条路径，**两条都绕不开辅助功能权限**：

**路径 A（主）**：`get_selected_text_accessibility()`（`clipboard.rs:197`）用 `osascript` 驱动 System Events 读取 `AXSelectedText`。

```198:211:src-tauri/src/commands/clipboard.rs
    let script = r#"
tell application "System Events"
    set frontApp to name of first application process whose frontmost is true
    tell process frontApp
        try
            set selectedText to value of attribute "AXSelectedText" of focused UI element
            return selectedText
        end try
    end tell
end tell
return ""
"#;

    let output = std::process::Command::new("osascript")
```

这条路实际要的是「自动操作 / Automation」权限（允许控制 System Events），而 AX 读取本身又要辅助功能权限。dev 模式下，这类苹果事件的授权往往被记到启动它的**终端**上，而不是 DH-TransShot。

**路径 B（回退）**：`simulate_cmd_c_via_cgevent()`（`clipboard.rs:53`）往 HID event tap 里 post 一个合成的 `Cmd+C`。

`CGEventPost` 属于「发送键盘事件」，需要辅助功能权限。

因此缺少辅助功能权限时，主路径和回退路径都会失败。

### 原因四：失败被吞掉，表现为静默失效

- `get_selected_text_accessibility()` 失败时只记日志，然后继续走回退路径。
- 回退路径检测不到剪贴板变化（`changeCount` 没变）时返回 `Ok(String::new())`，即**返回空串而不是错误**。
- 前端 `handleSelectedTextTranslate()`（`src/App.tsx:196`）拿到空串时只打一条 warn 就 `return`，连主窗口都不 `show`：

```207:211:src/App.tsx
      const text = await readSelectedText();
      if (!text.trim()) {
        appLog.warn("[App] 未获取到选中文本");
        return;
      }
```

结果：用户侧看到的就是「按了 Alt+Q 完全没反应」，没有任何可操作的提示。

### 次要放大因素

- `src-tauri/tauri.conf.json:48` 用的是自签名 `"signingIdentity": "DH-TransShot Dev"`，每次重新构建签名都会变，TCC 里之前给的授权条目就失效了，需要删掉重新添加。
- `tauri.conf.json:49` `"entitlements": null`，没有 hardened runtime 相关配置。
- dev 模式（`pnpm tauri dev`）从终端启动，权限通常要授给终端（Terminal / iTerm / VS Code）才生效。

## 因果链

```
macOS 27 把「辅助功能」改名为「设备控制和数据访问」
        │
        ├─► 用户按旧文档找不到授权入口
        │
        └─► 应用从未 AXIsProcessTrustedWithOptions 申请权限
                    │
                    └─► 应用不在权限列表里（也无法手动授权到位）
                                │
                                ├─► osascript + System Events 读取失败
                                └─► CGEventPost 模拟 Cmd+C 失败
                                            │
                                            └─► 回退返回空串 → 前端 warn 后 return
                                                        │
                                                        └─► 现象：Alt+Q 无反应，且无提示
```

## 手动恢复

1. 打开 **系统设置 → 隐私与安全性 → 设备控制和数据访问**（macOS ≤ 26 为「辅助功能」）。
2. 点 `+` 手动添加应用：
   - 正式安装：`/Applications/DH-TransShot.app`
   - dev 模式：`src-tauri/target/debug/dh-transshot`，或启动它的终端
3. 打开对应开关；若列表里已有旧条目，先 `−` 删除再加回来，然后重启应用。
4. 因为主路径走 `osascript` + System Events，还需在 **隐私与安全性 → 自动操作** 中允许 DH-TransShot（或终端）控制「System Events」。

## 核实（2026-09-25，macOS 27.0 / 26A428）

- `SecurityPrivacyExtension.appex` 的 `TCCServiceList.plist` 仍包含 `TCCServiceAccessibility`：权限服务没有删除
- 同一 bundle 的 `Localizable.loctable` 中 `ACCESSIBILITY` 键的显示名：en = **Device Control and Data Access**，zh_CN = **设备控制和数据访问**（是「和」不是「与」）
- 代码侧：修复前全仓库无 `AXIsProcessTrusted*` / `AXUIElement*` 调用，主路径为 `osascript`，回退路径缺权限时返回空串、前端静默 `return` —— 与上文分析一致

## 修复（v1.1.2）

对应原「修复方向」四项：

1. **主动申请权限**：`read_selected_text` 在 macOS 下先调用 `AXIsProcessTrustedWithOptions({kAXTrustedCheckOptionPrompt: true})`。未授权时系统把 DH-TransShot 登记进「设备控制和数据访问」列表并弹出授权框。只在按 `Alt+Q` 时申请，不在启动时打扰不用该功能的用户
2. **原生 AX 替换 osascript**：`clipboard.rs` 的 `ax` 模块用裸 FFI 调用 `AXUIElementCreateSystemWide` → `AXFocusedUIElement` → `AXSelectedText`，去掉对「自动操作」权限的依赖，权限归属到本应用；消息超时设为 1s。AX 读不到（浏览器/Electron 常见）时仍回退到 CGEvent 模拟 Cmd+C
3. **明确报错**：未授权时直接返回带设置路径的中文提示（不再走必然失败的回退）；前端 `handleSelectedTextTranslate` 捕获错误后写入 `translationStore.error` 并弹出主窗口
4. **文案**：README / README_EN / `Info.plist` / 本文档已补充 macOS 27 的新名称

**仍需注意：**

- 自签名（`signingIdentity: "DH-TransShot Dev"`）每次重新构建签名都会变，升级后若 `Alt+Q` 提示缺权限，需在列表里删除旧条目再重新添加
- dev 模式（`pnpm tauri dev`）下 TCC 通常把权限记在启动它的终端上，需给终端授权
- 系统授权框只会弹一次；用户拒绝后需按提示手动去设置里打开
- 已在本机（macOS 27.0）验证 FFI 链接与 CFString 往返（单测），授权弹窗与真实读取需在打包后的应用上手测

## 参考来源

- [What's new for enterprise in macOS Golden Gate 27 — Apple Support](https://support.apple.com/en-us/148830)
- [macOS 27 Accessibility Grant: What Breaks — mdm.tools](https://mdm.tools/blog/macos-27-accessibility-grant-removed/)
- [macOS 27 renames Accessibility permissions #1561 — boring.notch](https://github.com/TheBoredTeam/boring.notch/issues/1561)
