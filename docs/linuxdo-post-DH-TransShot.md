# LinuxDO 推广帖 — DH-TransShot

---

## 帖子标题

> 【开源推广】受不了Electron臃肿，用Tauri+Rust手搓了一个截图+翻译一体机，佬友试试

## 标签

开源推广, 开源项目, 开源, 软件开发, Tauri, Rust, 翻译, 截图

---

## 正文

本帖使用社区开源推广，符合推广要求。我申明并遵循社区要求的以下内容：

- 我的帖子已经打上 开源推广 标签： **是**
- 我的开源项目完整开源，无未开源部分： **是**
- 我的开源项目已链接认可 LINUX DO 社区： **是**
- 我帖子内的项目介绍，AI生成、润色内容部分已截图发出： **是**
- 以上选择我承诺是永久有效的，接受社区和佬友监督： **是**

以下为项目介绍正文内容，AI生成、润色内容已使用截图方式发出

---

### 开源推广

佬友们好，最近造了个小工具 **DH-TransShot**，截图+翻译一体化的桌面软件。

起因很简单：平时看文档、刷外文网页，截图→扔翻译→复制结果，一套操作下来贼烦。Bob 很好用但只有 macOS，Windows 上就只能用那些 Electron 套壳的，动不动吃几百兆内存。

忍不了了，直接上 **Tauri + Rust** 搞了一个，安装包才十几兆，内存占用极低，**彻底告别 Electron！**

### 功能一览

- **📸 截图标注**（`Alt+A`）：框选截图，支持矩形、箭头、画笔、文字标注，取色器按 `C` 一键复制色值
- **🌍 框选翻译**（`Alt+S`）：选区→OCR识别→AI翻译→结果面板，一条龙
- **📝 划词翻译**（`Alt+Q`）：选中文字直接翻译，走无障碍API
- **📋 剪贴板翻译**：标题栏点一下"T"，直接翻译剪贴板内容
- **🔊 TTS 朗读**：翻译结果可以念出来
- **🔌 接口通用**：OpenAI / DeepSeek / Ollama / SiliconFlow，兼容 OpenAI 协议的都能用
- **🎭 亮暗主题**：自动跟随系统

### 测试平台

已在以下平台实测通过：
- ✅ **macOS 26**（Tahoe）
- ✅ **macOS 15**（Sequoia）
- ✅ **Windows 10**

### 技术栈

| 层 | 技术 |
|---|---|
| 后端 | **Rust** + **Tauri v2** + Tokio |
| 前端 | React 19 + TypeScript + Tailwind CSS v4 |
| 状态 | Zustand / Mutex |
| 截图 | xcap (Rust crate) |
| OCR/翻译 | OpenAI 兼容 API |
| 构建 | GitHub Actions 自动打包 |

没有 Electron，没有 Node runtime，**纯 Rust 内核 + Webview 前端**，就是快就是轻。

### 这个项目怎么来的

说实话，这个项目是跟几个顶流 AI 一起 Vibe Coding 出来的：
- **Opus 4.6、4.7、4.8**
- **GPT 5.5**

特别感谢 **Any神**（@AnyContext）的指导和调试支持，没有他这个项目不可能这么快落地 🙏

### 安全说明

- 所有构建产物由 **GitHub Actions** 自动生成，你可以自己看 workflow 文件
- **无恶意代码**，完全开源，欢迎审计
- macOS 需要授权：屏幕录制（截图）、辅助功能（划词翻译）、输入监听（全局快捷键）

### 欢迎参与

积极接受 **PR** 和 **Issue**！不管是 bug 反馈、功能建议还是直接提代码，都欢迎。

目前在持续迭代中，有任何问题直接开 Issue，佬友们的反馈是最好的动力。

### 项目地址

🔗 **https://github.com/Danyhug/DH-TransShot**

下载直接去 Releases 页面，macOS 用 `.dmg`，Windows 用 `.msi`。

如果觉得好用的话，**点个 Star** 就是对我最大的支持 ⭐

---

## 使用的截图（发帖时附上）

发帖时从项目 `image/` 目录下载以下图片作为附件：

1. `https://raw.githubusercontent.com/Danyhug/DH-TransShot/main/image/image1.png` — 主界面翻译效果
2. `https://raw.githubusercontent.com/Danyhug/DH-TransShot/main/image/image2.png` — 设置界面

如果需要更多截图，可以从 Releases 中的安装包实际截图。

---

## AI 生成/润色声明截图说明

以上正文中"功能一览"表格、"技术栈"表格、"测试平台"列表为 AI 辅助润色内容，发帖时需截图留证。其余为本人手写。

> 📌 LinuxDO 原帖地址：[【开源推广】受不了Electron臃肿，用Tauri+Rust手搓了一个截图+翻译一体机，佬友试试](https://linux.do)  （发帖后替换为实际帖子链接）
