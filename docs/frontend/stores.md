# Zustand 状态管理（stores/）

## 概述

基于 Zustand 的前端状态管理层，提供响应式状态和 actions，替代 props 层层传递。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src/stores/translationStore.ts` | 翻译状态：源/目标文本、语言、翻译进度、错误 |
| `src/stores/settingsStore.ts` | 设置状态：服务配置（翻译/OCR/TTS）、弹窗开关 |
| `src/stores/logStore.ts` | 日志状态：日志条目列表、事件通信、openDebugWindow、appLog 辅助函数 |

## 核心逻辑

### translationStore.ts

**状态字段：**

| 字段 | 类型 | 初始值 | 说明 |
|------|------|--------|------|
| `sourceText` | string | `""` | 源文本 |
| `translatedText` | string | `""` | 翻译结果 |
| `sourceLang` | string | `"auto"` | 源语言代码 |
| `targetLang` | string | `"zh-CN"` | 目标语言代码 |
| `isTranslating` | boolean | `false` | 翻译进行中标志 |
| `isOcrProcessing` | boolean | `false` | OCR 识别进行中标志 |
| `error` | string \| null | `null` | 错误信息 |

**Actions：**
- `setSourceText(text)` / `setTranslatedText(text)` — 更新文本
- `setSourceLang(lang)` / `setTargetLang(lang)` — 更新语言
- `setIsTranslating(v)` — 切换翻译状态
- `setIsOcrProcessing(v)` — 切换 OCR 识别状态
- `setError(error)` — 设置错误信息
- **`swapLanguages()`** — 交换源/目标语言和源/目标文本
  - 若 `sourceLang === "auto"` 则直接 return（不可交换）
  - 同时交换 `sourceLang ↔ targetLang` 和 `sourceText ↔ translatedText`

### settingsStore.ts

**状态字段：**

| 字段 | 类型 | 初始值 | 说明 |
|------|------|--------|------|
| `settings` | Settings | 默认配置 | 完整用户设置 |

**默认值常量（导出）：**
- `emptyService` — 空 ServiceConfig（所有字段为空字符串）
- `defaultSettings` — 完整默认 Settings（复用 `emptyService`），可被其他文件导入

**默认 Settings：**
```typescript
{
  translation: { base_url: "", api_key: "", model: "", extra: "" },
  ocr: { base_url: "", api_key: "", model: "", extra: "" },
  tts: { base_url: "", api_key: "", model: "", extra: "" },
  source_language: "auto",
  target_language: "zh-CN",
  hotkey_screenshot: "Alt+A",
  hotkey_region: "Alt+S",
}
```

前端服务配置默认为空字符串，实际值在主窗口 `App.tsx` 初始化或监听 `settings-saved` 事件时通过 `getSettings()` 从后端获取。

**类型定义：**
- `ServiceName = "translation" | "ocr" | "tts"` — 服务名称联合类型

**Actions：**
- `setSettings(settings)` — 整体替换设置
- `updateService(service, key, value)` — 更新指定服务配置的单个字段
  - `service`: `"translation"` | `"ocr"` | `"tts"`
  - `key`: `ServiceConfig` 的字段名（`base_url` / `api_key` / `model` / `extra`）
  - `value`: 新值

## 依赖关系

- **外部依赖**：`zustand`
- **类型依赖**：`types/index.ts`（Settings、ServiceConfig）
- **被依赖**：
  - `translationStore` → `useTranslation` hook、`TranslationPanel`、`App.tsx`
  - `settingsStore` → `useTranslation` hook、`App.tsx`、`SettingsPanel`（导入 `defaultSettings`）

## 修改指南

- Zustand store 使用 `create<T>((set, get) => ({...}))` 模式
- 当前为纯内存状态，刷新丢失；如需持久化可使用 `zustand/middleware` 的 `persist`
- `swapLanguages` 使用 `get()` 获取当前值再 `set()`，避免闭包过期
- `updateService` 支持三种服务的字段更新，通过计算属性名 `[service]` 动态选择
- 新增 store 遵循 `use*Store` 命名，文件放在 `stores/` 目录
- Settings 的默认值需与后端 `config/settings.rs` 中的 Default impl 保持一致
- 新增服务类型时，需同时更新 `ServiceName` 类型和默认值

### logStore.ts

**状态字段：**

| 字段 | 类型 | 初始值 | 说明 |
|------|------|--------|------|
| `logs` | LogEntry[] | `[]` | 日志条目列表（最多 200 条） |

**LogEntry 结构：**
```typescript
{ id: number, time: string, level: "info" | "warn" | "error", message: string }
```

**Actions：**
- `addLog(level, message)` — 添加日志条目，超过 200 条时丢弃最早的；同时通过 `emitTo("debug-log", "debug-log-entry", entry)` 实时推送给调试窗口
- `clear()` — 清空所有日志，同时通知调试窗口清空

**导出函数：**
- `openDebugWindow()` — 打开（或 focus）调试窗口，吸附在主窗口右侧。使用 `lib/windowUtils.ts` 的 `openDockedWindow()` 通用函数
- `setupMainWindowLogListeners()` — 在主窗口侧设置事件监听（监听 `"debug-log-ready"` 后发送全量日志），返回 cleanup 函数

**全局辅助函数 `appLog`：**
- `appLog.info(msg)` / `appLog.warn(msg)` / `appLog.error(msg)`
- 同时调用 `console.*` 和 `addLog()`，可在任何文件中直接 import 使用
- 通过 `useLogStore.getState()` 访问 store，无需在 React 组件内使用
