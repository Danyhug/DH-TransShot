# TypeScript 类型定义（types/）

## 概述

前端共享类型定义，定义前后端通信的数据结构和事件载荷类型。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src/types/index.ts` | 所有 TypeScript 接口定义 |

## 核心逻辑

### Settings — 用户配置（对应后端 `config::Settings`）

```typescript
interface Settings {
  llm: LlmConfig;
  source_language: string;
  target_language: string;
  hotkey_screenshot: string;
  hotkey_region: string;
}
```

### LlmConfig — LLM 服务配置（对应后端 `config::LlmConfig`）

```typescript
interface LlmConfig {
  base_url: string;
  api_key: string;
  model: string;
}
```

### RegionSelectEvent — 区域选择事件载荷

```typescript
interface RegionSelectEvent {
  x: number;       // 物理像素 X 坐标
  y: number;       // 物理像素 Y 坐标
  width: number;   // 物理像素宽度
  height: number;  // 物理像素高度
  mode: string;    // "screenshot" | "ocr_translate"
}
```

- 由 `ScreenshotOverlay` emit `"region-selected"` 事件时携带
- `App.tsx` 中 `listen<RegionSelectEvent>("region-selected")` 接收

### ScreenshotInitEvent — 截图初始化事件载荷

```typescript
interface ScreenshotInitEvent {
  image: string;   // base64 编码的 PNG 全屏截图
  mode: string;    // "screenshot" | "ocr_translate"
}
```

- 由后端 `start_region_select` 命令 emit `"screenshot-init"` 时携带
- `ScreenshotOverlay` 中 `listen<ScreenshotInitEvent>("screenshot-init")` 接收

## 依赖关系

- **无外部依赖**
- **被依赖**：
  - `Settings` / `LlmConfig` → `stores/settingsStore.ts`、`lib/invoke.ts`
  - `RegionSelectEvent` → `App.tsx`
  - `ScreenshotInitEvent` → `components/screenshot/ScreenshotOverlay.tsx`

## 修改指南

- 类型定义使用 **snake_case** 字段名（匹配 Rust 后端的 serde 序列化输出）
- 修改 `Settings` 或 `LlmConfig` 时需同步更新：
  1. 后端 `config/settings.rs` 中的 Rust 结构体
  2. `stores/settingsStore.ts` 中的默认值
  3. `components/settings/SettingsDialog.tsx` 中的表单字段
- 新增事件类型遵循 `*Event` 后缀命名
- `mode` 字段当前为 `string` 类型，可考虑改为字面量联合类型 `"screenshot" | "ocr_translate"` 以获得更好的类型安全
