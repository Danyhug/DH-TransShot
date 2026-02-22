# 主题系统

## 概述

基于 CSS 变量 + `prefers-color-scheme` 媒体查询的主题系统，自动跟随系统深色/浅色模式切换。浅色模式采用浅灰色基调（#F5F5F5），营造现代简约的悬浮窗风格。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src/styles/globals.css` | Tailwind 导入、CSS 变量定义、全局样式重置、滚动条样式 |

## 核心逻辑

### CSS 变量

| 变量名 | 浅色模式 | 深色模式 | 用途 |
|--------|---------|---------|------|
| `--color-bg` | `#F5F5F5` | `#1f2937` | 页面背景（极浅灰色） |
| `--color-surface` | `#EAEAEC` | `#111827` | 卡片/输入框背景（稍深浅灰色） |
| `--color-border` | `#DCDCDC` | `#374151` | 边框、分割线、滚动条 |
| `--color-text` | `#1A1A1A` | `#f9fafb` | 主文本 |
| `--color-text-secondary` | `#999999` | `#9ca3af` | 次要文本、图标颜色 |
| `--color-primary` | `#3b82f6` | `#3b82f6` | 主色（蓝色，两种模式相同） |
| `--color-primary-hover` | `#2563eb` | `#60a5fa` | 主色悬停状态 |

### 设计语言

- **色彩**：极简，以灰、白、黑为主
- **卡片**：`rounded-2xl` 大圆角矩形，`--color-surface` 背景
- **图标**：中灰色（`--color-text-secondary`），统一 stroke 风格
- **交互**：按钮悬停使用 `hover:bg-black/5` 半透明效果
- **留白**：充分的 padding 和 gap，营造呼吸感

### 模式切换

```css
:root { /* 浅色模式变量 */ }

@media (prefers-color-scheme: dark) {
  :root { /* 深色模式变量覆盖 */ }
}
```

- 无需 JavaScript 控制，完全依赖 CSS 媒体查询
- Tailwind CSS v4 自定义 variant：`@custom-variant dark (&:is(.dark *))`

### 全局样式重置

```css
* { margin: 0; padding: 0; box-sizing: border-box; }
html, body, #root { height: 100%; width: 100%; overflow: hidden; }
```

- 所有元素重置 margin/padding，使用 border-box
- 根容器占满视口，隐藏溢出（固定窗口大小，无页面滚动）

### 字体

```css
font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
```

系统无衬线字体栈，macOS 使用 SF Pro，Windows 使用 Segoe UI，清晰易读。

### 滚动条

```css
::-webkit-scrollbar { width: 6px; }
::-webkit-scrollbar-track { background: transparent; }
::-webkit-scrollbar-thumb { background: var(--color-border); border-radius: 3px; }
```

- 6px 窄滚动条
- 透明轨道
- 滑块颜色跟随主题边框色

## 依赖关系

- **依赖**：`tailwindcss`（通过 `@import "tailwindcss"`）
- **被依赖**：`main.tsx` 和 `screenshot.tsx` 导入此文件

## 组件中的使用方式

组件通过 `style` 属性引用 CSS 变量：
```tsx
style={{ backgroundColor: "var(--color-bg)" }}
style={{ color: "var(--color-text-secondary)" }}
style={{ border: "1px solid var(--color-border)" }}
```

同时可与 Tailwind 工具类混用：
```tsx
className="flex items-center gap-2 px-4 py-3"
```

## 修改指南

- 新增颜色变量需在浅色和深色两个区块同时定义
- 组件中不要硬编码颜色值，始终使用 `var(--color-*)` 引用
- `--color-primary` 在两种模式下保持一致，如需差异化可分别设置
- `overflow: hidden` 在根容器上确保固定窗口无滚动，内部可滚动区域需单独设置
- Tailwind 自定义 variant `dark` 当前配置为 class-based（`.dark *`），但实际使用的是 media query，二者不冲突
- 浅色模式使用 #F5F5F5 外层背景 + #EAEAEC 卡片背景的双层灰色层级
