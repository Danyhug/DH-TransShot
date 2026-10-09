# 官网（website/）

## 概述

产品官网，单页静态站点，不经过 Vite 构建，也不依赖项目里的任何前端代码。通过 GitHub Pages 发布，地址为 <https://danyhug.github.io/DH-TransShot/>。

## 文件清单

| 文件 | 职责 |
|------|------|
| `website/index.html` | 整个页面：HTML / CSS / JS 全部内嵌 |
| `website/assets/*.webp` | 界面截图（主用，体积约为 PNG 的 1/3） |
| `website/assets/*.png` | 同名截图的 PNG 兜底，供不支持 WebP 的浏览器使用 |
| `.github/workflows/pages.yml` | 部署工作流 |

## 部署

`main` 分支上 `website/**` 或工作流文件有变动时自动部署，也可以在 Actions 页手动触发（`workflow_dispatch`）。工作流把 `website/` 目录原样上传为 Pages artifact，没有构建步骤。

首次使用需要在仓库 **Settings → Pages → Build and deployment → Source** 选择 **GitHub Actions**。

## 外部依赖

| 依赖 | 用途 | 加载方式 |
|------|------|---------|
| Google Fonts（Inter Tight / Instrument Serif / JetBrains Mono） | 拉丁字体 | `preload` + `onload` 异步，不阻塞首屏 |
| cdnjs three.js r128 | Hero 粒子 | `load` 事件后空闲时动态插入 |
| `api.github.com/repos/Danyhug/DH-TransShot` | 导航栏 Star 数 | 结果在 `localStorage` 缓存 1 小时，失败时只隐藏数字 |

中文一律使用系统字体（苹方 / 宋体 / 雅黑），不加载中文网页字体：中文字体按字符切片，整页会拉取数 MB，是首屏慢的主要原因。

## 主题

颜色全部定义为 `:root` 上的 CSS 变量，亮色为默认值，暗色在 `prefers-color-scheme: dark`（`:root:not([data-theme="light"])`）和 `:root[data-theme="dark"]` 下各定义一次。导航栏按钮手动切换后写入 `localStorage`（`transshot-theme`），`<head>` 内联脚本在首帧前恢复，避免闪烁。

强调色只有一个朱红（选区颜色），粒子颜色也从 `--p-base` / `--p-accent` / `--p-bg` 读取，主题切换时重新着色：亮色用正常混合，暗色用加法混合。暗色下截图叠加 `--shot-mask` 蒙版，悬停时减淡。

## 更新截图

截图源文件在仓库根目录 `image/`。替换后需要同时更新 `website/assets/` 下同名的 `.png` 和 `.webp`，并核对 `index.html` 中 `<img>` 的 `width` / `height`。
