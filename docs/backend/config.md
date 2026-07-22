# 配置与状态模块（config/）

## 概述

定义应用全局状态 `AppState` 和用户配置结构体 `Settings`，通过 Mutex 实现线程安全的共享可变状态。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/config/mod.rs` | 模块声明，公开导出 `AppState`、`Settings`、`HotkeyConfig`、`MonitorInfo`、`merge_extra` |
| `src-tauri/src/config/settings.rs` | 配置结构体定义、默认值和工具函数 |

## 核心逻辑

### settings.rs

**`ExtraProvider` — 额外模型提供商**

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | String | 显示名（用于 UI 区分） |
| `base_url` | String | API 基础 URL；为空时回退到全局 `Settings.base_url` |
| `api_key` | String | API 密钥；为空时回退到全局 `Settings.api_key` |
| `model` | String | 模型名称；为空时回退到 `ServiceConfig.model`（默认模型） |

**`ServiceConfig` — 服务配置（翻译/OCR/TTS 通用）**

| 字段 | 类型 | 说明 |
|------|------|------|
| `model` | String | 默认提供商使用的模型名称 |
| `extra` | String | JSON 字符串，合并到请求体（所有提供商共享，可覆盖 temperature 等参数） |
| `providers` | `Vec<ExtraProvider>` | 额外的模型提供商列表（默认空数组） |
| `active` | i32 | 当前生效的提供商索引：`-1`（默认值）= 使用全局 base_url+api_key+model；`0+` = `providers[active]` |

`ServiceConfig::with_model_and_extra(model, extra)` 工厂方法：设置 model 和 extra，providers 为空、active=-1。

`ServiceConfig::resolved(default_base_url, default_api_key) -> (String, String, String)` 解析当前生效的 `(base_url, api_key, model)`，根据 `active` 字段选择默认或某个额外提供商，并对额外提供商的空字段做全局回退。

**`Settings` — 完整用户配置**

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `base_url` | String | 环境变量 `DEFAULT_BASE_URL`，未设置时 `"https://api.siliconflow.cn"` | 全局共享 API 基础 URL（翻译/OCR/TTS 共用） |
| `api_key` | String | 环境变量 `DEFAULT_API_KEY`，未设置时 `""` | 全局共享 API 密钥（翻译/OCR/TTS 共用） |
| `translation` | ServiceConfig | model=`"tencent/Hunyuan-MT-7B"`, extra=`{"temperature":0.3, "top_p":0.9, "max_tokens":4096, "enable_thinking":false}` | 翻译服务配置 |
| `ocr` | ServiceConfig | model=`"Qwen/Qwen3.5-4B"`, extra=`{"temperature":0.1, "top_p":0.9, "max_tokens":4096, "enable_thinking":false}` | OCR 服务配置 |
| `tts` | ServiceConfig | model=`"FunAudioLLM/CosyVoice2-0.5B"`, extra=`{"voice":"...:alex", "speed":1.0, "response_format":"mp3", "sample_rate":44100, "enable_thinking":false}` | TTS 服务配置 |
| `hotkeys` | HotkeyConfig | `screenshot="Alt+A"`, `ocr_translate="Alt+S"`, `clipboard_translate="Alt+Q"` | 三个动作的快捷键字符串，使用 `Alt+A`、`Ctrl+Shift+S`、`Cmd+K` 等格式（由 `tauri_plugin_global_shortcut::Shortcut::from_str` 解析） |

**`base_url` 端点自适应拼接（`api_client::build_endpoint_url`）**

`base_url` 不要求填完整端点，后端会根据填写形态自动拼出正确请求地址（Chat Completions 拼 `chat/completions`，TTS 拼 `audio/speech`）。规则按优先级：

| 优先级 | 用户填写形态 | 处理 | 示例（Chat） |
|--------|-------------|------|-------------|
| 0 | 以 `#` 结尾 | 去掉 `#` 后**原样请求**（raw 模式，支持自定义/非标准路径） | `http://x.top/my/api#` → `http://x.top/my/api` |
| 1 | 已含完整端点 | 原样使用 | `https://api.openai.com/v1/chat/completions` → 不变 |
| 2 | 末尾是版本段（`v1`/`v4`/`v1beta`…） | 追加 `/端点`，保留原版本号 | `https://open.bigmodel.cn/api/paas/v4` → `…/v4/chat/completions` |
| 3 | 其余（根地址） | 追加 `/v1/端点` | `https://api.openai.com` → `…/v1/chat/completions` |

- 版本段判定：段以 `v` 开头且紧跟数字（`is_version_segment`）
- 翻译/OCR 经 `chat_completions_url()` → `build_endpoint_url(base_url, "chat/completions")`；TTS 经 `audio_speech_url()` → `build_endpoint_url(base_url, "audio/speech")`
- **注意**：全局 `base_url` 被三个服务共享回退，不要用 `#` 或完整端点把它锁死成某一个端点（会导致其它服务取不到自己的端点）；完整端点/`#` 建议用在各服务自己的 `provider.base_url` 上
- 前端设置界面已在「API 地址」输入下展示该规则摘要

**`HotkeyConfig` — 快捷键配置**

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `screenshot` | String | `"Alt+A"` | 区域截图快捷键 |
| `ocr_translate` | String | `"Alt+S"` | 区域翻译快捷键 |
| `clipboard_translate` | String | `"Alt+Q"` | 翻译选中文本快捷键 |

- 字符串使用 `+` 分隔，修饰键支持 `Alt`/`Option`/`Ctrl`/`Control`/`Shift`/`Cmd`/`Command`/`Super`/`CmdOrCtrl`，主键支持 `A-Z`、`0-9`、`F1-F24`、`Space`、`Enter`、`Tab`、`Escape`、方向键、标点符号等
- 每个字段使用 `#[serde(default = "...")]`，旧版 settings.json（无 `hotkeys` 字段）反序列化时自动填充默认值
- 修改后由 `save_settings` 触发 `hotkey::reload_hotkeys` 立即生效

- `base_url` 和 `api_key` 字段使用 `#[serde(default)]`，旧版 settings.json（无顶层 base_url/api_key）能正常反序列化并回退到默认值
- `ServiceConfig.providers` 默认空数组、`active` 默认 -1，旧版 settings.json（无这两个字段）能正常反序列化并保持默认行为
- 所有结构体实现 `Serialize`、`Deserialize`、`Clone`

**`merge_extra(body, extra, tag)` — 请求体合并工具函数**

将 `extra` JSON 字符串解析后合并到 `body`（`serde_json::Value`）中，extra 中的 key 覆盖 body 中的同名 key。用于让用户自定义 temperature、top_p 等请求参数。

- `extra` 为空字符串时直接返回
- `extra` 不是 JSON 对象时打 warn 日志并忽略
- `tag` 参数用于日志前缀（如 `"Translation"`、`"OCR"`）

**`AppState` — 应用全局状态**

```rust
pub struct AppState {
    pub settings: Mutex<Settings>,
    pub frozen_screenshots: Mutex<Vec<String>>,
    pub frozen_mode: Mutex<String>,
    pub frozen_window_rects: Mutex<serde_json::Value>,
    pub frozen_monitors: Mutex<Vec<serde_json::Value>>,
    pub tts_cache: Mutex<TtsCache>,
    pub http_client: reqwest::Client,
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `settings` | `Mutex<Settings>` | 用户配置，所有命令共享读写 |
| `frozen_screenshots` | `Mutex<Vec<String>>` | 区域选择流程中逐显示器冻结的截图（每个元素为该显示器的 base64 PNG） |
| `frozen_mode` | `Mutex<String>` | 区域选择模式（`"screenshot"` / `"ocr_translate"`） |
| `frozen_window_rects` | `Mutex<serde_json::Value>` | 冻结的窗口矩形列表（JSON 数组） |
| `frozen_monitors` | `Mutex<Vec<serde_json::Value>>` | 冻结的显示器信息列表（MonitorInfo JSON） |
| `tts_cache` | `Mutex<TtsCache>` | TTS 内存缓存，命中后直接返回已合成的 base64 音频 |
| `http_client` | `reqwest::Client` | 共享 HTTP 客户端（连接池复用），供 OCR 和翻译模块使用 |

## 环境变量配置

项目根目录的 `.env` 文件用于存放共用的默认配置，不提交到版本控制：

```
DEFAULT_BASE_URL=https://api.siliconflow.cn
DEFAULT_API_KEY=sk-your-api-key
```

- `.env` — 真实开发配置（在 `.gitignore` 中排除）
- `.env.test` — 测试用占位配置（提交到仓库）

`lib.rs` 在 `run()` 函数最前面调用 `dotenvy::dotenv()` 加载环境变量，随后 `Settings::default()` 通过 `std::env::var()` 读取顶层 `base_url` 和 `api_key`。

每个服务有独立的默认配置（翻译：`tencent/Hunyuan-MT-7B`，OCR：`Qwen/Qwen3.5-4B`，TTS：`FunAudioLLM/CosyVoice2-0.5B`），且包含优化过的 extra 参数默认值。

## 依赖关系

- **外部依赖**：`serde`（序列化/反序列化）、`serde_json`（JSON 操作）、`std::sync::Mutex`、`reqwest`（HTTP 客户端）、`dotenvy`（.env 加载）、`log`（日志）
- **被依赖**：
  - `lib.rs` 创建并注册 `AppState`
  - `commands/screenshot.rs` 读写 `frozen_screenshots`、`frozen_mode`、`frozen_window_rects`、`frozen_monitors`
  - `commands/translation.rs` 读取 `settings.base_url`、`settings.api_key`、`settings.translation`、`http_client`
  - `commands/ocr.rs` 读取 `settings.base_url`、`settings.api_key`、`settings.ocr`、`http_client`
  - `commands/settings.rs` 读写 `settings`
  - `commands/tts.rs` 读取 `settings`、`tts_cache`、`http_client`
  - `translation/openai_compat.rs` 使用 `merge_extra`
  - `ocr/mod.rs` 使用 `merge_extra`

## 修改指南

- `Settings` 的字段变更需同步更新前端 `src/types/index.ts` 和 `src/stores/settingsStore.ts` 的默认值
- `AppState` 使用 `std::sync::Mutex`（非 `tokio::sync::Mutex`），不可跨 `.await` 持锁
- Settings 通过 `tauri_plugin_store` 持久化到 `settings.json`（路径：`~/Library/Application Support/com.danyhug.dh-transshot/settings.json`）
  - 启动时在 `lib.rs` 的 `setup()` 中从 store 加载已保存的配置
  - `save_settings` 命令在更新内存状态后同步写入 store 文件
  - 旧版 settings.json（含 `llm` 字段）无法反序列化，会自动回退到默认配置
- 新增全局共享状态字段需添加到 `AppState`，并在 `Default` impl 中初始化
- `frozen_screenshots` 存储每个显示器的完整 base64 字符串，多显示器时占用大量内存
- `tts_cache` 当前为进程内内存缓存，容量固定 64 条；涉及 TTS 输出参数的变更应考虑是否清空缓存或调整缓存键
- **禁止将 API Key 硬编码到源码中**，必须通过 `.env` 文件或用户设置界面配置
- 新增服务类型时，在 `Settings` 中添加对应的 `ServiceConfig` 字段，并更新前端类型和 UI
