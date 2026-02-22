# 工具库（lib/）

## 概述

前端工具函数层，封装 Tauri invoke 调用和语言数据定义。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src/lib/invoke.ts` | 类型化的 Tauri invoke 命令封装 |
| `src/lib/languages.ts` | 支持的语言列表定义 |
| `src/lib/windowUtils.ts` | 子窗口管理工具（吸附式窗口创建/focus） |

## 核心逻辑

### invoke.ts - Tauri 命令封装

对 `@tauri-apps/api/core` 的 `invoke()` 进行类型化封装，确保前后端接口类型安全。

| 函数 | 参数 | 返回值 | 对应后端命令 |
|------|------|--------|-------------|
| `startRegionSelect(mode)` | `mode: string` | `Promise<void>` | `start_region_select` |
| `captureRegion(x, y, width, height)` | 4 个 number | `Promise<string>` | `capture_region` |
| `recognizeText(imageBase64, language)` | 2 个 string | `Promise<string>` | `recognize_text` |
| `translateText(text, sourceLang, targetLang)` | 3 个 string | `Promise<string>` | `translate_text` |
| `getSettings()` | — | `Promise<Settings>` | `get_settings` |
| `saveSettings(settings)` | `settings: Settings` | `Promise<void>` | `save_settings` |
| `readClipboard()` | — | `Promise<string>` | `read_clipboard` |
| `copyImageToClipboard(imageBase64)` | `imageBase64: string` | `Promise<void>` | `copy_image_to_clipboard` |

**注意：** Tauri invoke 的参数名使用 camelCase，Tauri 会自动转换为后端的 snake_case。

### languages.ts - 语言列表

**Language 接口：**
```typescript
interface Language { code: string; name: string }
```

**支持的 15 种语言：**

| code | name |
|------|------|
| `auto` | Auto Detect |
| `zh-CN` | Chinese (Simplified) |
| `zh-TW` | Chinese (Traditional) |
| `en` | English |
| `ja` | Japanese |
| `ko` | Korean |
| `fr` | French |
| `de` | German |
| `es` | Spanish |
| `pt` | Portuguese |
| `ru` | Russian |
| `ar` | Arabic |
| `it` | Italian |
| `th` | Thai |
| `vi` | Vietnamese |

**导出：**
- `languages` — 包含 `auto` 的完整列表（用于源语言选择）
- `targetLanguages` — 过滤掉 `auto` 的列表（用于目标语言选择）

## 依赖关系

- **依赖**：`@tauri-apps/api/core`（invoke）、`@tauri-apps/api/window`（getCurrentWindow）、`@tauri-apps/api/webviewWindow`（WebviewWindow）、`types/index.ts`（Settings）
- **被依赖**：
  - `invoke.ts` → `hooks/useScreenshot`、`hooks/useTranslation`、`App.tsx`、`SettingsPanel`、`LogPanel`
  - `languages.ts` → `components/translation/LanguageSelector.tsx`
  - `windowUtils.ts` → `stores/logStore.ts`（openDebugWindow）、`stores/settingsStore.ts`（openSettingsWindow）

## 修改指南

- 新增 Tauri 命令时同步添加 invoke 封装函数，保持类型安全
- invoke 参数名必须与后端 `#[tauri::command]` 函数参数名的 camelCase 形式一致
- 新增语言需同时更新 `languages` 数组，并确认后端 OCR 模块支持该语言
- `auto` 语言仅适用于源语言，`targetLanguages` 会自动排除
