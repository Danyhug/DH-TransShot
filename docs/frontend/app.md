# 主窗口编排（App.tsx + ScreenshotApp.tsx）

## 概述

前端核心编排层。`App.tsx` 是主窗口的根组件，负责监听后端事件、路由操作、串联截图-OCR-翻译工作流。`ScreenshotApp.tsx` 是截图覆盖层窗口的根组件。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src/App.tsx` | 主窗口根组件：事件监听、操作路由、工作流编排 |
| `src/ScreenshotApp.tsx` | 截图覆盖层根组件：包装 ScreenshotOverlay |
| `src/main.tsx` | 主窗口 React 入口（渲染 App 到 #root） |
| `src/screenshot.tsx` | 覆盖层 React 入口（渲染 ScreenshotApp 到 #root） |

## 核心逻辑

### App.tsx

**事件监听（useEffect）：**

| 事件 | 处理逻辑 |
|------|---------|
| `onFocusChanged` | 失焦时关闭 settings/debug-log 子窗口后隐藏主窗口（非置顶且焦点未转到子窗口时） |
| `region-selected` | 根据 mode 执行 OCR+翻译流程或仅截图 |
| `tray-action` | 路由到 `handleAction()` |
| `hotkey-action` | 路由到 `handleAction()` |

**`handleAction(action)` 路由：**
- `"screenshot"` → `startRegion("screenshot")`
- `"ocr_translate"` → `startRegion("ocr_translate")`
- `"clipboard_translate"` → `handleClipboardTranslate()`（模拟复制选中文字 → 读取剪贴板 → 翻译 → 显示主窗口）

**`handleAction` 不再提前 show/focus 主窗口** — 主窗口的显示由覆盖层关闭回调根据 mode 决定。

**region-selected 事件处理（根据 mode 分支）：**

**screenshot 模式：**
1. `captureRegion(x, y, width, height)` — 裁切选区图片
2. `copyImageToClipboard(imageBase64)` — 复制到剪贴板
3. 主窗口不弹出

**ocr_translate 模式：**
1. `captureRegion(x, y, width, height)` — 裁切选区图片
2. `recognizeText(imageBase64, sourceLang)` — OCR 识别
3. `setSourceText(ocrText)` — 填入源文本框
4. 若文本非空：`translate(ocrText)` — LLM 翻译
5. 主窗口由覆盖层关闭回调恢复位置 + show + focus

**UI 布局：**
```
┌──────────────────────────────┐
│ 📌            ✂️  📷  ⚙️      │  ← TitleBar：左 pin，右侧功能图标
├──────────────────────────────┤
│ ┌──────────────────────────┐ │
│ │ 源文本输入                │ │  ← 源文本卡片（圆角、surface 背景）
│ │ 🔊 📋                    │ │  ← ActionButtons 在卡片内底部
│ └──────────────────────────┘ │
│    英语 ▼   ⇄   中文简体 ▼   │  ← 语言选择行（居中）
│        🔲 OpenAI ▼           │  ← 服务选择器（居中）
│ ┌──────────────────────────┐ │
│ │ 翻译结果                  │ │  ← 翻译结果卡片
│ │ 🔊 📋                    │ │  ← ActionButtons 在卡片内底部
│ └──────────────────────────┘ │
│  SettingsDialog (modal)      │  ← 设置弹窗（条件渲染）
└──────────────────────────────┘
```

**操作入口：**
- 标题栏图标按钮触发 OCR 翻译、区域截图、设置
- 全局快捷键和托盘菜单通过事件系统触发
- Ctrl/Cmd+Enter 快捷键触发翻译

**useEffect 依赖：** `[]` — 初始化时注册一次事件监听

### ScreenshotApp.tsx

- 仅渲染 `<ScreenshotOverlay />` 组件
- 作为 `screenshot.html` 的独立 React 应用入口

### main.tsx / screenshot.tsx

- 标准 React 入口文件，`ReactDOM.createRoot` 渲染到 `#root`
- 均启用 `StrictMode`

## 依赖关系

- **依赖**：
  - `hooks/useScreenshot`、`hooks/useTranslation`
  - `stores/translationStore`、`stores/settingsStore`
  - `lib/invoke`（captureRegion、recognizeText、copyImageToClipboard）
  - `components/*`（TitleBar、TranslationPanel、SettingsDialog、ScreenshotOverlay）
- **被依赖**：`main.tsx` / `screenshot.tsx` 作为 Vite 入口

## 修改指南

- 新增操作类型需在 `handleAction` 的 switch 中添加分支
- 新增事件监听需在 `useEffect` 中注册 `listen()`，并在 cleanup 中调用 unlisten
- 工作流中的错误处理通过 `setError()` 反映到 UI
- `useEffect` 的依赖数组为 `[]`（初始化时注册一次事件监听）
- TitleBar 通过 props 回调接收操作函数（`onScreenshot`、`onOcrTranslate`、`onDebugLog`、`onSettings`）
