# UI 组件（components/）

## 概述

前端 UI 组件层，按功能域划分为翻译面板、截图覆盖层、设置弹窗和通用组件四个子目录。采用现代简约的悬浮卡片设计，浅灰色背景 + 圆角矩形卡片布局，大量留白，清晰层级感。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src/components/translation/TranslationPanel.tsx` | 翻译面板主组件：卡片化布局（源文本 → 语言选择 → 翻译结果含服务头部） |
| `src/components/translation/LanguageSelector.tsx` | 语言下拉选择器（透明背景、可见下拉箭头、中文语言名） |
| `src/components/translation/SwapButton.tsx` | 源/目标语言互换按钮 |
| `src/components/translation/TextArea.tsx` | 通用文本域（透明背景，由外层卡片提供样式） |
| `src/components/translation/ActionButtons.tsx` | 朗读 + 复制按钮（内嵌于卡片底部） |
| `src/components/screenshot/ScreenshotOverlay.tsx` | 全屏截图覆盖层：冻结截图背景 + 拖拽选区 |
| `src/components/settings/SettingsDialog.tsx` | 设置弹窗：LLM 配置 + 快捷键展示 |
| `src/components/debug/LogPanel.tsx` | 调试日志面板：日志列表 + 剪贴板内容 + 操作按钮 |
| `src/components/common/TitleBar.tsx` | 自定义标题栏：左侧 Pin 置顶 + 右侧功能图标（相机、裁切框、日志、开关） |

## 核心逻辑

### TranslationPanel.tsx

**布局结构（卡片化设计）：**
```
┌─────────────────────────────┐
│ TextArea(source, editable)   │  ← 源文本卡片（surface 背景、rounded-2xl）
│ ActionButtons(source)        │  ← 喇叭 + 复制 在卡片内底部
└─────────────────────────────┘
  LanguageSelector ⇄ LanguageSelector  ← 居中行（带下拉箭头）
┌─────────────────────────────┐
│ ServiceSelector(header)      │  ← 彩色 Logo + 服务名 + 下拉
│ TextArea(result, readOnly)   │  ← 翻译结果卡片
│ ActionButtons(result)        │  ← 喇叭 + 复制 在卡片内底部
└─────────────────────────────┘
[Error message]
```

- 文本区域包裹在 `rounded-2xl` 卡片容器中，背景色为 `--color-surface`
- ServiceSelector 内嵌于翻译结果卡片顶部作为头部标题栏
- ActionButtons 在卡片容器内部底部显示
- 翻译中时结果区显示 "翻译中..." 文本
- 快捷键 `Ctrl/Cmd+Enter` 触发翻译
- 外层容器 `px-4 pb-4 pt-1 gap-3`，充分留白

### TitleBar.tsx

**布局：**
- 左侧：Pin 图钉图标按钮（切换窗口置顶，使用 `appWindow.setAlwaysOnTop()`）
- 右侧四个图标按钮：
  - 相机（区域截图 ⌥A）
  - 裁切框（区域翻译 ⌥S）
  - 文件文本（调试日志）
  - 开关/滑块（设置）
- 标题栏无背景色（透明，继承外层 `--color-bg`）
- 保留 `data-tauri-drag-region` 实现窗口拖拽
- Pin 激活时图标变为主题色 + 填充样式
- 按钮悬停效果：`hover:bg-black/5`

**Props：**
- `onScreenshot` — 区域截图按钮回调
- `onOcrTranslate` — 区域翻译按钮回调
- `onDebugLog` — 调试日志按钮回调
- `onSettings` — 设置按钮回调

### ScreenshotOverlay.tsx

**状态：**
- `backgroundImage` — 冻结截图 base64
- `mode` — 操作模式（"screenshot" / "ocr_translate"）
- `windowRects` — 后端采集的可见窗口矩形列表（逻辑坐标）
- `isSelecting` — 是否正在选区操作（mousedown 后）
- `isDragging` — 是否已开始拖拽（移动距离 >= 5px）
- `selRect` — 拖拽选区矩形
- `hoveredRect` — 鼠标悬停时匹配到的窗口矩形

**交互流程：**
1. mount 时通过 `getFrozenScreenshot()` 获取冻结截图、mode 和 `window_rects`
2. **悬停检测**（mousedown 前的 mousemove）：遍历 `windowRects`（前到后顺序），找到第一个包含光标的窗口，设置 `hoveredRect` 显示绿色高亮
3. **mousedown** → 记录起点，开始选区
4. **mousemove（拖拽中）** → 若移动距离 >= 5px，切换为拖拽模式（`isDragging=true`），清除窗口高亮，更新选区矩形
5. **mouseup 点击**（距离 < 5px）→ 使用 `hoveredRect` 作为选中区域，乘以 DPR 转物理像素，emit `"region-selected"`，关闭窗口
6. **mouseup 拖拽**（距离 >= 5px）→ 使用拖拽选区（最小 5×5），乘以 DPR 转物理像素，emit `"region-selected"`，关闭窗口
7. `ESC` → 直接关闭窗口

**视觉效果：**
- 30% 黑色半透明覆盖层
- **窗口高亮**：`border: 2px solid #22c55e`（绿色边框）+ `background: rgba(34,197,94,0.08)`（淡绿色填充）+ `box-shadow: 0 0 0 9999px` 镂空效果
- **拖拽选区**：蓝色边框 + `box-shadow: 0 0 0 9999px` 遮罩镂空效果
- 选区上方显示物理像素尺寸
- 开始拖拽后窗口高亮消失
- 无选区且无悬停窗口时显示操作提示

### SettingsPanel.tsx

- 独立设置窗口（非模态弹窗）
- 标签页切换：翻译 / OCR / TTS 服务配置
- 表单字段：API 地址、API 密钥（password）、模型、自定义参数
- "截图时隐藏主界面" toggle 开关（`hide_on_capture`）
- 快捷键参考区（只读展示）
- 保存时 emit `"settings-saved"` 事件通知主窗口刷新配置

### LogPanel.tsx

- **独立调试窗口**的完整内容组件（非模态浮层），运行在 `debug-log` 窗口中（`debug.html` → `debug.tsx` → `DebugApp.tsx` → `LogPanel`）
- 通过 Tauri 事件系统与主窗口通信：
  - mount 时 emit `"debug-log-ready"` 请求全量日志
  - 监听 `"debug-log-init"` 接收初始日志
  - 监听 `"debug-log-entry"` 接收增量日志
  - 监听 `"debug-log-clear"` 响应清空
- **布局（上下分区）**：顶部拖拽标题栏 + 关闭按钮 → 剪贴板内容区 → 日志列表（flex-1 填充） → 底部操作栏
- **日志列表**：带时间戳（HH:MM:SS）和 level 颜色区分（info=灰、warn=黄、error=红），可滚动，自动滚到底部
- **剪贴板内容**：窗口打开时用 `navigator.clipboard.readText()` 读取并展示
- **操作按钮**：「清除」清空本地日志列表、「复制全部」将日志复制到剪贴板
- 窗口宽度 360px，高度与主窗口一致，吸附在主窗口右侧

### LanguageSelector.tsx

- Props：`value`、`onChange`、`includeAuto`
- 透明背景，可见下拉箭头（SVG chevron）
- 使用 `relative` 容器 + `absolute` 定位箭头图标
- 渲染 `languages` 列表（中文语言名，来自 `lib/languages.ts`）
- `includeAuto` 控制是否包含 "自动检测" 选项

### SwapButton.tsx

- Props：`onClick`、`disabled`
- 源语言为 "auto" 时 disabled
- 双向箭头图标，secondary 文本色
- 悬停效果：`hover:bg-black/5`

### TextArea.tsx

- Props：`value`、`onChange`（可选）、`placeholder`、`readOnly`
- 透明背景（由外层卡片容器提供背景色）
- `flex-1` 填充可用空间
- `leading-relaxed` 行高，`px-3.5 py-3` 内边距

### ActionButtons.tsx

- **Copy**：`navigator.clipboard.writeText()`
- **Speak**：调用后端 `synthesizeSpeech`（`lib/invoke.ts`）通过 OpenAI 兼容的 `/v1/audio/speech` 接口合成语音，使用设置中配置的 TTS 模型
  - 收到 base64 音频后，构建 `data:audio/mp3;base64,...` URL，用 `new Audio(url).play()` 播放
  - 请求中按钮 disabled，防止重复点击
  - 错误通过 `appLog.error()` 记录
- 14px 图标尺寸，`px-3 pb-2.5` 内边距
- 文本为空或正在朗读时 disabled（opacity-25）
- 悬停效果：`hover:bg-black/5`

## 依赖关系

- **依赖**：
  - `hooks/useTranslation`
  - `stores/translationStore`、`stores/settingsStore`（defaultSettings）
  - `lib/languages`（语言列表，中文名称）
  - `lib/invoke`（getSettings、saveSettings、readClipboard、synthesizeSpeech）
  - `@tauri-apps/api/event`（listen、emit）
  - `@tauri-apps/api/window`（getCurrentWindow）
- **被依赖**：`App.tsx`、`ScreenshotApp.tsx`

## 修改指南

- 所有组件使用 CSS 变量（`var(--color-*)`）实现主题，避免硬编码颜色
- 卡片容器使用 `rounded-2xl` + `overflow-hidden` + `--color-surface` 背景
- TextArea 使用透明背景，样式由外层卡片控制
- ScreenshotOverlay 的 DPI 处理是关键：选区逻辑坐标 × DPR = 物理像素
- TitleBar 的 Pin 功能使用 Tauri `setAlwaysOnTop()` API
- ActionButtons 中的 TTS 通过后端 `synthesize_speech` 命令调用 OpenAI 兼容的 TTS API，使用设置中配置的模型
- 按钮悬停统一使用 `hover:bg-black/5` 半透明效果
