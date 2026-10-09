# 官网（website/）

## 概述

产品官网，单页静态站点，不经过 Vite 构建，也不依赖项目里的任何前端代码。通过 GitHub Pages 发布，地址为 <https://danyhug.github.io/DH-TransShot/>。

## 文件清单

| 文件 | 职责 |
|------|------|
| `website/index.html` | 整个页面：HTML / CSS / JS 全部内嵌 |
| `website/assets/*.webp` | 界面截图（主用，体积约为 PNG 的 1/3） |
| `website/assets/*.png` | 同名截图的 PNG 兜底，供不支持 WebP 的浏览器使用 |
| `website/favicon.svg` / `favicon-32.png` | 浏览器标签页图标（SVG 为主，PNG 兜底） |
| `website/apple-touch-icon.png` | iOS 添加到主屏幕时的图标（180×180，满版无圆角，由系统裁切） |
| `.github/workflows/pages.yml` | 部署工作流 |

## 部署

`main` 分支上 `website/**` 或工作流文件有变动时自动部署，也可以在 Actions 页手动触发（`workflow_dispatch`）。工作流把 `website/` 目录原样上传为 Pages artifact，没有构建步骤。

首次使用需要在仓库 **Settings → Pages → Build and deployment → Source** 选择 **GitHub Actions**。

## 外部依赖

| 依赖 | 用途 | 加载方式 |
|------|------|---------|
| Google Fonts（Inter Tight / Instrument Serif / JetBrains Mono） | 拉丁字体 | `preload` + `onload` 异步，不阻塞首屏 |
| `api.github.com/repos/Danyhug/DH-TransShot` | 导航栏 Star 数 | 结果在 `localStorage` 缓存 1 小时，失败时只隐藏数字 |

Hero 粒子是页面内联脚本直接调用 WebGL 绘制的（一个着色器程序、一个顶点缓冲，`gl.POINTS` 画点），没有使用 three.js：原先从 cdnjs 加载 three.js 在国内网络下要 2～6 秒，粒子总是比文字晚出现很久。每帧在 CPU 上算好透视投影后的屏幕坐标和点径再上传，透视参数沿用 three.js 版本（FOV 50°、相机 z=9），粒子大小与 Logo 落点保持一致。

中文一律使用系统字体（苹方 / 宋体 / 雅黑），不加载中文网页字体：中文字体按字符切片，整页会拉取数 MB，是首屏慢的主要原因。

## 主题

颜色全部定义为 `:root` 上的 CSS 变量，亮色为默认值，暗色在 `prefers-color-scheme: dark`（`:root:not([data-theme="light"])`）和 `:root[data-theme="dark"]` 下各定义一次。导航栏按钮手动切换后写入 `localStorage`（`transshot-theme`），`<head>` 内联脚本在首帧前恢复，避免闪烁。

强调色只有一个朱红（选区颜色），粒子颜色也从 `--p-base` / `--p-accent` / `--p-bg` 读取，主题切换时重新着色：亮色用正常混合，暗色用加法混合。界面截图分亮暗两套，元素加 `only-light` / `only-dark` 类，跟随页面主题（系统设置或手动切换）只显示其中一套；另一套为 `display:none`，配合 `loading="lazy"` 不会被下载。

## 更新截图

`website/assets/` 下每张截图都有 `-light` 和 `-dark` 两个版本，各带 `.png` 与 `.webp`。

- **深色**：当前版本的应用，在 Retina 屏上按窗口截取的 2x 原图（`screencapture -x -o -l <窗口ID>`，不带阴影）
- **浅色**：旧版截图（设置侧栏只有服务 / 快捷键 / 朗读三项），所以第 3 个标签浅色是「TTS 多提供商」，深色是「翻译偏好」，标签文字也随主题切换

| 浅色 | 深色 | 内容 |
|------|------|------|
| `main-light` | `main-dark` | 主窗口 |
| `settings-service-light` | `settings-service-dark` | 设置 → 服务 |
| `settings-tts-light` | `settings-translate-dark` | 设置 → 服务 → TTS 提供商 / 设置 → 翻译 |
| `settings-speech-light` | `settings-speech-dark` | 设置 → 朗读 |
| `settings-hotkey-light` | `settings-hotkey-dark` | 设置 → 快捷键 |
| `debug-light` | `debug-dark` | 主窗口 + 调试日志（并排拼接，中间留 20px 透明间隙） |

替换时同时更新同名的 `.png` 和 `.webp`，并核对 `index.html` 中 `<img>` 的 `width` / `height`。截图里不要出现真实的 API 地址、密钥或模型路由名。

## 图标

App 图标、官网 favicon 与导航栏 Logo 是同一个图形：四角取景框 + T，右下角那一角用强调色朱红。源文件是仓库根目录的 `icon.svg`（1024×1024，macOS 图标网格：824×824 圆角方块、四周留 100px）。

- **App 图标**：在仓库根目录运行 `pnpm tauri icon icon.svg`，重新生成 `src-tauri/icons/` 下所有平台的图标。托盘也直接使用其中的 `32x32.png`
- **官网**：`website/favicon.svg` 是 `icon.svg` 的副本，`favicon-32.png` 复制自 `src-tauri/icons/32x32.png`；`apple-touch-icon.png` 是去掉留白和圆角的满版版本
- 根目录的 `icon.png` 是 `icon.svg` 渲染出的 1024px PNG，供不支持 SVG 的场景使用
