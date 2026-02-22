# 自定义 Hooks（hooks/）

## 概述

封装业务逻辑的 React 自定义 Hooks，连接 Zustand stores 和 Tauri invoke 调用，为组件提供简洁的 API。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src/hooks/useScreenshot.ts` | 截图操作封装（区域选择触发） |
| `src/hooks/useTranslation.ts` | 翻译逻辑封装（输入验证、API 调用、状态管理） |

## 核心逻辑

### useScreenshot.ts

**返回值：** `{ startRegion }`

**`startRegion(mode = "screenshot")`**
- 调用 `startRegionSelect(mode)` invoke
- 触发后端创建覆盖层窗口
- 失败仅 appLog.error

### useTranslation.ts

**返回值：** `{ sourceText, translatedText, sourceLang, targetLang, isTranslating, error, setSourceText, translate }`

**`translate(text?)`**
1. 取 `text` 参数或 store 中的 `sourceText`
2. 验证文本非空
3. 验证 API key 已配置（base_url 含 localhost 时可跳过，适配 Ollama）
4. `setIsTranslating(true)` + `setError(null)`
5. 调用 `translateText(input, sourceLang, targetLang)`
6. 成功 → `setTranslatedText(result)`
7. 失败 → `setError(String(e))`
8. finally → `setIsTranslating(false)`

**状态来源：**
- `useTranslationStore` — 翻译相关状态
- `useSettingsStore` — LLM 配置（用于验证 API key）

## 依赖关系

- **依赖**：
  - `stores/translationStore`、`stores/settingsStore`
  - `lib/invoke`（startRegionSelect、translateText）
  - `@tauri-apps/api/event`（emit）
- **被依赖**：
  - `useScreenshot` → `App.tsx`
  - `useTranslation` → `TranslationPanel.tsx`

## 修改指南

- `useTranslation` 的 `useCallback` 依赖数组需包含所有闭包引用的状态，遗漏会导致使用过期值
- API key 验证逻辑：仅在 base_url 不含 `localhost` 时要求 API key，新增 Provider 时可能需调整
- `useScreenshot` 当前错误处理仅 appLog.error，如需 UI 反馈需添加 error 状态
- 新增 hook 遵循 `useXxx` 命名，放在 `hooks/` 目录
