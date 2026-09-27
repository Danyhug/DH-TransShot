# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

DH-TransShot：截屏 + 翻译二合一桌面工具（macOS / Windows）。后端 Rust + Tauri v2（Tokio），前端 React 19 + TypeScript + Tailwind CSS v4 + Zustand，包管理用 pnpm。OCR、翻译、TTS 全部走 OpenAI 兼容接口（OCR 用视觉大模型）。

`AGENT.md` 是指向本文件的符号链接。

## 常用命令

```bash
pnpm tauri dev                  # 开发模式运行（后端日志输出到此终端）
pnpm tauri build                # 构建生产版本
pnpm exec tsc --noEmit          # TypeScript 类型检查
pnpm exec vite build            # 仅构建前端

# 以下在 src-tauri/ 目录下执行
cargo check
cargo fmt                       # 允许运行，整个 crate 的格式化 diff 可以保留
cargo test --lib                # Rust 单元测试（测试写在各模块的 #[cfg(test)] 中）
cargo test --lib <测试名>        # 运行单个测试，如 cargo test --lib tts::
cargo test --lib -- --ignored --test-threads=1   # audio 真机用例（需输出设备，灌静音不发声）
```

前端没有测试框架和 linter，校验靠 `tsc`。

开发配置：根目录 `.env`（不提交）提供 `DEFAULT_BASE_URL` / `DEFAULT_API_KEY`，启动时由 `dotenvy` 加载，作为所有服务的默认值；持久化的 `settings.json` 优先级更高。`.env.test` 是提交到仓库的占位配置。

## 架构要点

完整说明见 `docs/architecture.md`，以下是需要跨文件才能看明白的部分。

**多窗口**：Vite 四入口（`index.html` / `screenshot.html` / `settings.html` / `debug.html`），分别对应 `main.tsx→App.tsx`、`screenshot.tsx→ScreenshotApp.tsx`、`settings.tsx→SettingsApp.tsx`、`debug.tsx→DebugApp.tsx`。
- 主窗口常驻，失焦即隐藏
- 截图覆盖层由 `start_region_select` 命令动态创建，每块显示器一个，选区完成或 ESC 后销毁
- **销毁任何窗口都必须走 `window_lifecycle`（先 `hide()` 让帧再 `close()`）**：macOS 上直接 `close()` 还活着的 webview 会让 WebKit 的 display link 偶发访问已释放的滚动树，整个 App 段错误退出且崩溃栈上没有本项目的帧。前端对应 `closeWindowDeferred()`，见 `docs/backend/window_lifecycle.md`
- 设置窗口打开期间通过 `suspend_hotkeys` / `resume_hotkeys` 挂起全局快捷键
- 调试日志窗口吸附在主窗口右侧，展示前端 `appLog` 日志

**事件流**：快捷键（`hotkey.rs`）和托盘（`tray.rs`）都只 emit `hotkey-action` / `tray-action`（载荷为 `"screenshot"` / `"ocr_translate"` / `"clipboard_translate"`），由 `App.tsx` 的 `handleAction` 统一路由。覆盖层选区完成后 emit `region-selected`（物理像素坐标 + mode + monitor_index，截图模式可能带标注后的 `annotatedImage`），主窗口监听后再调用 `capture_region` / `capture_and_ocr` / `translate_text`。设置保存后 emit `settings-saved` 通知主窗口重载。

**冻结截图**：`start_region_select` 先对所有显示器截图并存入 `AppState`（`frozen_screenshots` / `frozen_monitors` / `frozen_window_rects`），覆盖层显示的是冻结图，后续裁切也基于冻结图，而不是重新截屏。

**DPI**：xcap 使用物理像素，前端使用逻辑像素。覆盖层窗口按 `scale_factor` 换算逻辑尺寸创建；`ScreenshotOverlay` emit 时按冻结图实际尺寸与 CSS 尺寸之比换算回物理像素。改选区/裁切逻辑时要特别注意两套坐标。

**后端状态**：`config/` 中的 `AppState` 通过 `tauri::manage()` 注册，命令通过 `State<AppState>` 访问，包含 `Mutex<Settings>`、冻结截图、TTS 缓存、共享 `reqwest::Client`、`audio::AudioOutput`。`Settings` 有三个 `ServiceConfig`（translation / ocr / tts），每个可挂多个 `ExtraProvider`，留空字段回退到服务级或全局值。命令层统一用 `ServiceConfig::resolved()` 取最终生效的 base_url / api_key / model / extra，不要自己拼回退逻辑。`api_client.rs` 封装共享的 Chat Completions 请求。

**TTS / 音频**：`tts/` 只负责合成，`audio/`（rodio/cpal）负责本地播放。`speak_text` 合成后直接把 PCM 送进系统输出设备，IPC Channel 上只回传 `{event:"start"}`，音频数据不经过前端。不要把播放挪回 WebView：窗口隐藏后 WebKit 会让 `AudioContext` 空转，没有任何报错却完全无声。前端 `lib/tts.ts` + `stores/ttsStore.ts` 只做朗读编排和状态管理。

**前后端 RPC**：所有 Tauri 命令在 `lib.rs` 的 `generate_handler!` 中注册，前端统一通过 `src/lib/invoke.ts` 封装调用。新增命令时两边都要改，权限配置在 `src-tauri/capabilities/default.json`。

## 开发规范

### 文档驱动开发

每个模块的设计文档在 `docs/` 下：`docs/architecture.md`（整体架构）、`docs/backend/<模块>.md`、`docs/frontend/<模块>.md`、`docs/theme.md`（CSS 变量主题，对应 `styles/globals.css`）。

1. 修改某个模块前，先读对应的 `docs/*.md`；跨模块变更前，先读 `docs/architecture.md`
2. 功能完成后同步更新对应文档；新增模块时新建对应文档，格式与现有文档保持一致
3. 需要拆分变更时分多次 commit，例如代码/格式化改动和 Markdown 文档改动分开提交

### 日志规范

前后端统一使用 `[模块名]` 前缀，方便对照排查。日志中带上关键参数（语言、文本长度、区域坐标、数据大小、HTTP 状态码、model/base_url），但不要输出完整的长文本、base64 或 api_key。

- **前端**：使用 `appLog.info/warn/error`（来自 `stores/logStore.ts`）。关键操作都要记录：函数入口、异步操作前后、错误捕获、分支判断。`logStore.ts` 内部用 `console.log`，避免递归。已有前缀：`[App]` `[Screenshot]` `[Overlay]` `[Translate]` `[Settings]`
- **后端**：使用 `log` crate 的 `info!` / `warn!` / `error!`。已有前缀：`[Setup]` `[Screenshot]` `[Capture]` `[OCR]` `[Translation]` `[Settings]` `[Hotkey]` `[Tray]` `[TTS]` `[Audio]`
- 级别：info 用于正常流程节点；warn 用于可处理的异常（输入为空、选区过小、配置缺失、API Key 为空）；error 用于失败和异常

```typescript
appLog.warn("[Overlay] 选区太小 (" + width + "x" + height + ")，已忽略");
```
```rust
info!("[Translation] 发送请求到 {}, model={}", url, model);
```

### OCR 优化约定

瓶颈主要在图像体积、上传耗时和模型视觉编码，不在 Rust 本地逻辑。

1. 只把用户框选的区域送去 OCR，不要送整屏截图
2. 把 OCR 输入的最长边限制在约 2048px，不要直接上传 Retina 原图
3. 无透明通道时优先编码为 JPEG
4. base64 解码、缩放、裁切、重编码都是 CPU 密集型操作，必须放进 `spawn_blocking`
5. OCR prompt 只要求输出识别出的文字，不加解释或结构化包装
6. 调模型参数优先用 `ocr.extra` 的顶层兼容字段（如 `max_tokens`）；改嵌套视觉字段要谨慎，别覆盖默认的 `messages` 结构
7. 日志只记录分辨率、base64 大小、耗时、model、base_url

## 全局快捷键

默认 `Alt+A` 区域截图（框选 → 标注 → 复制到剪贴板），`Alt+S` 区域翻译（框选 → OCR → 翻译），`Alt+Q` 翻译选中文本（优先用 Accessibility API 读取，失败时回退为模拟复制并恢复剪贴板）。标题栏的「T」按钮翻译剪贴板内容。快捷键可在设置中修改，保存后立即生效，字符串由 `tauri_plugin_global_shortcut::Shortcut::from_str` 解析，详见 `docs/backend/hotkey.md`。

## 发版流程

用户说「发版」时：

1. 确认工作区干净，功能改动已提交
2. 运行 `pnpm release`（patch），需要时用 `pnpm release:minor` / `pnpm release:major`，脚本会创建发版提交和 annotated tag
3. 按脚本输出执行 `git push && git push origin v<版本号>`
4. GitHub Actions（`.github/workflows/release.yml`，macOS aarch64 + Windows）会根据 tag 同步 `package.json`、`tauri.conf.json`、`Cargo.toml`、`Cargo.lock` 的版本号再打包

版本号以 git tag 为准，不要只手动改其中一个文件；需要指定版本时运行 `pnpm sync-version v<版本号>`。打错 tag 时用 `git tag -d v<版本号>` 和 `git push origin --delete v<版本号>` 删除。
