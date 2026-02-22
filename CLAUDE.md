# DH-TransShot

截屏+翻译二合一桌面工具，支持 macOS 和 Windows。

## 技术栈

- **后端**: Rust + Tauri v2
- **前端**: React 19 + TypeScript + Tailwind CSS v4
- **状态管理**: Zustand
- **截图**: xcap
- **OCR**: 系统原生（macOS Vision / Windows Media.Ocr）
- **翻译**: OpenAI 兼容接口（支持 OpenAI、DeepSeek、Ollama 等）
- **包管理**: pnpm

## 开发规范（重要）

### 分层文档驱动开发

本项目采用子模块文档体系，所有模块的详细设计文档存放在 `docs/` 目录。

**必须遵守以下规范：**

1. **修改任何模块前，必须先阅读对应的 `docs/*.md` 文件**，了解模块职责、核心逻辑、依赖关系和修改注意事项
2. **跨模块变更前，必须先阅读 `docs/architecture.md`** 了解整体架构、事件系统和工作流
3. **新增功能完成后，必须同步更新对应的 `docs/*.md` 文件**，保持文档与代码一致
4. **新增模块时，必须创建对应的 `docs/*.md` 文件**，遵循统一文档格式

### 文档优先查阅顺序

- 不了解项目 → 先读 `docs/architecture.md`
- 修改后端某模块 → 先读 `docs/backend/<模块>.md`
- 修改前端某模块 → 先读 `docs/frontend/<模块>.md`
- 涉及主题/样式 → 先读 `docs/theme.md`

### 日志规范

前后端统一使用 `[模块名]` 前缀格式记录日志，方便关联排查。

#### 前端日志

使用 `appLog`（来自 `stores/logStore.ts`），日志会同步显示在独立调试窗口中。

**规范：**

1. **所有关键操作必须有日志**：函数入口、异步操作前后、错误捕获、分支判断
2. **使用 `[模块名]` 前缀**：
   - `[App]` — 主窗口编排（App.tsx）
   - `[Screenshot]` — 截图 hook
   - `[Overlay]` — 截图覆盖层
   - `[Translate]` — 翻译 hook
   - `[Settings]` — 设置 hook
   - 新增模块时自定义前缀，保持简短
3. **日志级别**：
   - `appLog.info()` — 正常流程节点（开始、完成、状态变更）
   - `appLog.warn()` — 非预期但可处理的情况（输入为空、选区过小、配置缺失）
   - `appLog.error()` — 操作失败、异常捕获
4. **携带关键参数**：日志消息中包含有助于排查的上下文值（语言、文本长度、区域坐标、数据大小等），但避免输出完整的大段文本或 base64
5. **logStore 内部用 `console.log`**：在 `logStore.ts` 自身的函数中（如 `openDebugWindow`）使用 `console.log` 而非 `appLog`，避免递归

**示例：**
```typescript
appLog.info("[Translate] 手动翻译: " + sourceLang + " → " + targetLang + ", 文本长度=" + input.length);
appLog.warn("[Overlay] 选区太小 (" + width + "x" + height + ")，已忽略");
appLog.error("[Settings] 配置保存失败: " + String(e));
```

#### 后端日志

使用 `log` crate 的 `info!` / `warn!` / `error!` 宏，日志输出到终端（`pnpm tauri dev` 可见）。

**规范：**

1. **同样使用 `[模块名]` 前缀**：
   - `[Setup]` — 应用启动初始化（lib.rs）
   - `[Screenshot]` — 截图命令层（commands/screenshot.rs）
   - `[Capture]` — 截图底层实现（screenshot/capture.rs）
   - `[OCR]` — OCR 识别（commands/ocr.rs + ocr/mod.rs）
   - `[Translation]` — 翻译（commands/translation.rs + translation/openai_compat.rs）
   - `[Settings]` — 配置读写（commands/settings.rs）
   - `[Hotkey]` — 快捷键（hotkey.rs）
   - `[Tray]` — 系统托盘（tray.rs）
2. **日志级别**：
   - `info!` — 命令入口、API 请求/响应状态、操作完成
   - `warn!` — 配置缺失、API Key 为空等非致命情况
   - `error!` — API 错误、截图失败、序列化/持久化失败
3. **携带关键参数**：region 坐标、base64 大小、HTTP 状态码、model/base_url 等
4. **禁止输出敏感信息**：不要在日志中输出完整的 api_key

**示例：**
```rust
info!("[Screenshot] start_region_select, mode={}", mode);
info!("[Translation] 发送请求到 {}, model={}", url, model);
error!("[OCR] API 错误 ({}): {}", status, body);
```

## 子模块文档索引

### 架构层

| 文档 | 内容 |
|------|------|
| [docs/architecture.md](docs/architecture.md) | 整体架构、核心工作流、多窗口架构、事件系统、DPI 处理、模块依赖总览 |

### 后端模块（src-tauri/src/）

| 文档 | 对应代码 | 内容 |
|------|---------|------|
| [docs/backend/entry.md](docs/backend/entry.md) | `lib.rs` + `main.rs` | Tauri Builder 入口、插件注册、命令注册 |
| [docs/backend/commands.md](docs/backend/commands.md) | `commands/` | Tauri 命令层（前后端 RPC 接口） |
| [docs/backend/screenshot.md](docs/backend/screenshot.md) | `screenshot/` | xcap 截图捕获、base64 编码、区域裁切 |
| [docs/backend/ocr.md](docs/backend/ocr.md) | `ocr/` | OCR 识别（macOS Vision FFI / Windows Media.Ocr） |
| [docs/backend/translation.md](docs/backend/translation.md) | `translation/` | OpenAI 兼容 Chat Completions 翻译客户端 |
| [docs/backend/config.md](docs/backend/config.md) | `config/` | Settings 结构体、AppState 全局状态 |
| [docs/backend/tray.md](docs/backend/tray.md) | `tray.rs` | 系统托盘菜单与事件路由 |
| [docs/backend/hotkey.md](docs/backend/hotkey.md) | `hotkey.rs` | 全局快捷键注册与事件发射 |

### 前端模块（src/）

| 文档 | 对应代码 | 内容 |
|------|---------|------|
| [docs/frontend/app.md](docs/frontend/app.md) | `App.tsx` + `ScreenshotApp.tsx` | 主窗口编排、事件监听、工作流路由 |
| [docs/frontend/components.md](docs/frontend/components.md) | `components/` | UI 组件（翻译面板、截图覆盖层、设置弹窗、标题栏） |
| [docs/frontend/hooks.md](docs/frontend/hooks.md) | `hooks/` | 自定义 Hooks（截图、翻译、设置） |
| [docs/frontend/stores.md](docs/frontend/stores.md) | `stores/` | Zustand 状态管理（翻译状态、设置状态） |
| [docs/frontend/lib.md](docs/frontend/lib.md) | `lib/` | Tauri invoke 封装、语言列表 |
| [docs/frontend/types.md](docs/frontend/types.md) | `types/` | TypeScript 类型定义 |

### 主题

| 文档 | 对应代码 | 内容 |
|------|---------|------|
| [docs/theme.md](docs/theme.md) | `styles/globals.css` | CSS 变量主题、深色/浅色模式、全局样式 |

## 项目结构

```
src-tauri/src/
├── lib.rs                      # Tauri Builder 入口
├── main.rs                     # 程序入口
├── commands/                   # Tauri 命令层（前后端 RPC 接口）
│   ├── screenshot.rs
│   ├── ocr.rs
│   ├── translation.rs
│   └── settings.rs
├── screenshot/                 # 截图捕获
│   └── capture.rs
├── ocr/                        # OCR 识别（平台分发）
│   ├── apple_vision.rs
│   └── windows_ocr.rs
├── translation/                # LLM 翻译
│   └── openai_compat.rs
├── config/                     # 配置与全局状态
│   └── settings.rs
├── tray.rs                     # 系统托盘
└── hotkey.rs                   # 全局快捷键

src/
├── App.tsx                     # 主窗口编排
├── ScreenshotApp.tsx           # 截图覆盖层
├── components/                 # UI 组件
│   ├── translation/
│   ├── screenshot/
│   ├── settings/
│   └── common/
├── hooks/                      # 业务逻辑 Hooks
├── stores/                     # Zustand 状态管理
├── lib/                        # 工具函数
├── types/                      # TypeScript 类型
└── styles/                     # 全局样式
```

## 全局快捷键

| 快捷键 | 功能 |
|--------|------|
| `Alt+A` (macOS: `⌥A`) | 区域截图（框选 → 裁切 → 复制到剪贴板） |
| `Alt+S` (macOS: `⌥S`) | 区域翻译（框选 → 裁切 → OCR → 翻译 → 显示） |

## 常用命令

```bash
pnpm tauri dev          # 开发模式运行
pnpm tauri build        # 构建生产版本
pnpm exec tsc --noEmit  # TypeScript 类型检查
pnpm exec vite build    # 仅构建前端
cargo check             # 仅检查 Rust 编译（需在 src-tauri/ 目录下）
```

## 构建产物

- `src-tauri/target/release/bundle/macos/DH-TransShot.app`
- `src-tauri/target/release/bundle/dmg/DH-TransShot_0.1.0_aarch64.dmg`
