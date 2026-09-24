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
  base_url: string;
  api_key: string;
  translation: ServiceConfig;
  ocr: ServiceConfig;
  tts: ServiceConfig;
  hotkeys: HotkeyConfig;
  speech: SpeechConfig;
  translation_prompt: TranslationPromptConfig;
}

interface TranslationPromptConfig {
  expand_abbreviations: boolean; // 解释缩写（单独缩写 → 候选列表；句中缩写 → 按上下文意译）
  domains: string[];             // 行业偏向 key，多选；空 = 通用
  custom_prompt: string;         // 自定义附加翻译指令
  base_prompt: string;           // 编辑过的内置翻译规则；空 = 内置默认
}

interface ServiceConfig {
  model: string;
  /** 共享自定义参数：默认提供商 + 所有未单独填写 extra 的提供商都用它 */
  extra: string;
  providers: ExtraProvider[];
  /** -1 = 默认（使用顶层 base_url/api_key + model）；0+ = providers 索引 */
  active: number;
}

interface SpeechConfig {
  auto_read_source: boolean;  // 翻译后自动朗读原文
  auto_read_target: boolean;  // 翻译后自动朗读译文
  stream_playback: boolean;   // 流式边收边播（小米 MiMo chat+audio）
}

interface ExtraProvider {
  name: string;
  base_url: string;
  api_key: string;
  model: string;
  /** 该提供商专用参数；留空继承 ServiceConfig.extra，填写则整体覆盖 */
  extra: string;
}
```

### RegionSelectEvent — 区域选择事件载荷

```typescript
interface RegionSelectEvent {
  x: number;               // 物理像素 X 坐标
  y: number;               // 物理像素 Y 坐标
  width: number;           // 物理像素宽度
  height: number;          // 物理像素高度
  mode: string;            // "screenshot" | "ocr_translate"
  monitor_index: number;   // 显示器索引
  annotatedImage?: string; // 标注后的图片 base64（仅 screenshot 标注模式）
}
```

- 由 `ScreenshotOverlay` emit `"region-selected"` 事件时携带
- `App.tsx` 中 `listen<RegionSelectEvent>("region-selected")` 接收

### WindowRect — 窗口矩形

```typescript
interface WindowRect {
  x: number;
  y: number;
  width: number;
  height: number;
}
```

- 由后端 `list_window_rects()` 返回，逻辑坐标（points）

### MonitorInfo — 显示器信息

```typescript
interface MonitorInfo {
  name: string;
  x: number;           // 物理像素位置
  y: number;           // 物理像素位置
  width: number;       // 物理像素尺寸
  height: number;      // 物理像素尺寸
  scale_factor: number;
}
```

### ScreenshotInitEvent — 截图初始化数据（通过 invoke 获取）

```typescript
interface ScreenshotInitEvent {
  image: string;               // base64 编码的 PNG 冻结截图
  mode: string;                // "screenshot" | "ocr_translate"
  window_rects: WindowRect[];  // 可见窗口矩形列表
  monitors: MonitorInfo[];     // 显示器信息列表
}
```

- 由前端通过 `getFrozenScreenshot(monitorIndex)` invoke 命令获取（非事件推送）
- `ScreenshotOverlay` mount 时调用获取截图、mode、窗口矩形和显示器信息

## 依赖关系

- **无外部依赖**
- **被依赖**：
  - `Settings` → `stores/settingsStore.ts`、`lib/invoke.ts`
  - `RegionSelectEvent` → `App.tsx`
  - `ScreenshotInitEvent` → `lib/invoke.ts`、`components/screenshot/ScreenshotOverlay.tsx`
  - `WindowRect` / `MonitorInfo` → `components/screenshot/ScreenshotOverlay.tsx`

## 修改指南

- 类型定义使用 **snake_case** 字段名（匹配 Rust 后端的 serde 序列化输出）
- 修改 `Settings` 时需同步更新：
  1. 后端 `config/settings.rs` 中的 Rust 结构体
  2. `stores/settingsStore.ts` 中的默认值
  3. `components/settings/SettingsPanel.tsx` 中的表单字段
- 新增事件类型遵循 `*Event` 后缀命名
- `mode` 字段当前为 `string` 类型，可考虑改为字面量联合类型 `"screenshot" | "ocr_translate"` 以获得更好的类型安全
