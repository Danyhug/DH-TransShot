# 整体架构

## 概述

DH-TransShot 是截屏+翻译二合一桌面工具，采用 Tauri v2 多窗口架构，后端 Rust 处理截图/OCR/翻译，前端 React 负责 UI 交互。

## 技术栈

| 层 | 技术 |
|---|---|
| 后端运行时 | Rust + Tauri v2 + Tokio |
| 前端框架 | React 19 + TypeScript |
| 样式 | Tailwind CSS v4 + CSS 变量主题 |
| 状态管理 | Zustand（前端）/ Mutex\<T\>（后端） |
| 截图 | xcap crate |
| OCR | 视觉大模型（OpenAI 兼容 API） |
| 翻译 | OpenAI 兼容 Chat Completions API |
| 构建 | Vite 多入口 + Cargo |
| 包管理 | pnpm |

## 多窗口架构

```
┌─────────────────────────────────┐
│  主窗口 (index.html)            │
│  常驻 · 480×680 · 无边框        │
│  翻译面板 + 底部工具栏           │
└──────────┬──────────────────────┘
           │ Tauri 事件系统
┌──────────▼──────────────────────┐
│  截图覆盖层 (screenshot.html)    │
│  动态创建/销毁 · 全屏 · 置顶     │
│  拖拽选区 → emit 坐标后自关闭    │
└─────────────────────────────────┘
```

- **主窗口**：应用启动即存在，承载翻译面板和设置界面
- **截图覆盖层**：由 `start_region_select` 命令动态创建，用户选区完成或按 ESC 后销毁
- Vite 配置了 `index.html` + `screenshot.html` 双入口构建

## 事件系统

窗口间通过 Tauri 事件系统（`emit` / `listen`）通信：

| 事件名 | 方向 | 载荷 | 用途 |
|--------|------|------|------|
| `screenshot-init` | 后端 → 覆盖层 | `{ image: base64, mode: string }` | 传递冻结截图和操作模式 |
| `region-selected` | 覆盖层 → 主窗口 | `{ x, y, width, height, mode }` | 传递选区坐标（物理像素） |
| `hotkey-action` | 后端 → 前端 | `string`（"screenshot"/"ocr_translate"） | 全局快捷键触发 |
| `tray-action` | 后端 → 前端 | `string`（同上） | 托盘菜单触发 |

## 核心工作流

### 区域截图（Alt+A / ⌥A）

```
快捷键触发
  → hotkey.rs emit("hotkey-action", "screenshot")
  → App.tsx handleAction("screenshot")
  → useScreenshot.startRegion("screenshot")
  → commands/screenshot.rs start_region_select()
    → capture_full() 冻结屏幕
    → 存入 AppState.frozen_screenshot
    → 创建覆盖层窗口
  → ScreenshotOverlay 显示冻结截图，用户拖拽选区
  → emit("region-selected") 传递物理像素坐标 + mode，关闭覆盖层
  → App.tsx 监听 region-selected (mode="screenshot")
    → captureRegion() 裁切图片
    → copyImageToClipboard() 复制到剪贴板
    → 主窗口不弹出
```

### 区域翻译（Alt+S / ⌥S）

```
快捷键触发
  → hotkey.rs emit("hotkey-action", "ocr_translate")
  → App.tsx handleAction("ocr_translate")
  → useScreenshot.startRegion("ocr_translate")
  → commands/screenshot.rs start_region_select()
    → capture_full() 冻结屏幕
    → 存入 AppState.frozen_screenshot
    → 创建覆盖层窗口
  → ScreenshotOverlay 显示冻结截图，用户拖拽选区
  → emit("region-selected") 传递物理像素坐标 + mode，关闭覆盖层
  → App.tsx 监听 region-selected (mode="ocr_translate")
    → captureRegion() 裁切图片
    → recognizeText() OCR 识别
    → setSourceText() 填入源文本
    → translate() 执行翻译
    → 主窗口恢复位置 + show + focus
```

## DPI 处理

xcap 使用**物理像素**，前端使用**逻辑像素**。

- 覆盖层窗口：后端获取 `monitor.scale_factor()`，用物理尺寸除以缩放因子计算逻辑尺寸创建窗口
- 选区坐标：前端 `ScreenshotOverlay` 在 emit 时将逻辑坐标乘以 `window.devicePixelRatio` 转为物理像素
- 尺寸指示器：显示物理像素尺寸（逻辑尺寸 × DPR）

## 后端状态管理

`AppState` 通过 `tauri::manage()` 注册为全局状态，所有 Tauri 命令通过 `State<AppState>` 访问：

```rust
pub struct AppState {
    pub settings: Mutex<Settings>,           // 用户配置
    pub frozen_screenshot: Mutex<Option<String>>,  // 区域选择期间的冻结截图
    pub frozen_mode: Mutex<String>,          // 区域选择模式
    pub http_client: reqwest::Client,        // 共享 HTTP 客户端（连接池复用）
}
```

`Settings` 包含三个 `ServiceConfig`（translation / ocr / tts），每个服务独立配置 base_url、api_key、model 和 extra（自定义 JSON 参数）。

## 环境变量

敏感配置（API Key 等）通过项目根目录的 `.env` 文件管理，启动时由 `dotenvy` 加载：

```
DEFAULT_BASE_URL=https://api.siliconflow.cn
DEFAULT_API_KEY=sk-your-api-key
```

base_url 和 api_key 由所有服务（翻译/OCR/TTS）共用作为默认值，各服务的 model 有独立的硬编码默认值。

- `.env` — 真实开发配置（`.gitignore` 排除，不提交）
- `.env.test` — 测试用占位配置（提交到仓库）

加载优先级：持久化配置（settings.json）> `.env` 默认值

## 前端状态管理

两个 Zustand store 管理前端状态：

- `translationStore`：源文本、翻译结果、语言选择、翻译状态
- `settingsStore`：服务配置（翻译/OCR/TTS 各自的 base_url/api_key/model/extra）、设置弹窗开关

## 模块依赖关系总览

```
lib.rs（入口）
  ├── commands/（Tauri 命令层 - RPC 接口）
  │     ├── screenshot → screenshot/capture
  │     ├── ocr → ocr/（平台分发）
  │     ├── translation → translation/openai_compat
  │     └── settings → config/settings
  ├── config/（应用状态 + 配置结构体）
  ├── tray.rs（系统托盘 → emit 事件）
  └── hotkey.rs（全局快捷键 → emit 事件）

App.tsx（前端入口 - 事件编排）
  ├── hooks/（业务逻辑封装）
  │     ├── useScreenshot → lib/invoke
  │     ├── useTranslation → stores + lib/invoke
  │     └── useSettings → stores + lib/invoke
  ├── stores/（Zustand 状态）
  ├── components/（UI 组件）
  └── lib/invoke（Tauri 命令调用封装）
```

## macOS 代码签名（开发环境）

macOS 的屏幕录制权限绑定到应用的代码签名。如果使用 ad-hoc 签名（`signingIdentity: "-"`），每次编译都会生成不同的签名，macOS 会认为是新应用，需要重新授权屏幕录制权限。

**解决方案：** 使用本地自签名代码签名证书，保持每次编译签名一致。

### 创建证书（仅需执行一次）

```bash
# 1. 生成私钥
openssl genrsa -out /tmp/dh-dev.key 2048

# 2. 创建证书配置
cat > /tmp/dh-dev.conf << 'EOF'
[req]
distinguished_name = req_dn
x509_extensions = codesign
prompt = no

[req_dn]
CN = DH-TransShot Dev

[codesign]
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
EOF

# 3. 生成自签名证书（有效期 10 年）
openssl req -new -x509 -key /tmp/dh-dev.key -out /tmp/dh-dev.crt -days 3650 -config /tmp/dh-dev.conf

# 4. 打包为 PKCS12（-legacy 兼容 macOS 钥匙串）
openssl pkcs12 -export -in /tmp/dh-dev.crt -inkey /tmp/dh-dev.key -out /tmp/dh-dev.p12 -passout pass:temp123 -legacy

# 5. 导入钥匙串
security import /tmp/dh-dev.p12 -k ~/Library/Keychains/login.keychain-db -T /usr/bin/codesign -P "temp123"

# 6. 设置证书为代码签名可信
security find-certificate -c "DH-TransShot Dev" -p ~/Library/Keychains/login.keychain-db > /tmp/dh-dev.pem
security add-trusted-cert -p codeSign -k ~/Library/Keychains/login.keychain-db /tmp/dh-dev.pem

# 7. 清理临时文件
rm -f /tmp/dh-dev.key /tmp/dh-dev.crt /tmp/dh-dev.conf /tmp/dh-dev.p12 /tmp/dh-dev.pem

# 8. 验证
security find-identity -v -p codesigning
# 应输出：1) ... "DH-TransShot Dev"
```

### Tauri 配置

`tauri.conf.json` 中 `bundle.macOS.signingIdentity` 设为证书名称：

```json
"macOS": {
  "signingIdentity": "DH-TransShot Dev"
}
```

## 常用命令

```bash
pnpm tauri dev          # 开发模式运行
pnpm tauri build        # 构建生产版本
pnpm exec tsc --noEmit  # TypeScript 类型检查
pnpm exec vite build    # 仅构建前端
cargo check             # 仅检查 Rust 编译（需在 src-tauri/ 目录下）
```

## 构建产物

- macOS: `src-tauri/target/release/bundle/macos/DH-TransShot.app`
- DMG: `src-tauri/target/release/bundle/dmg/DH-TransShot_0.1.0_aarch64.dmg`
