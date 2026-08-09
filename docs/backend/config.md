# 配置与状态模块（config/）

## 概述

定义应用全局状态 `AppState` 和用户配置结构体 `Settings`，通过 Mutex 实现线程安全的共享可变状态。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/config/mod.rs` | 模块声明，公开导出 `AppState`、`Settings`、`HotkeyConfig`、`MonitorInfo`、`merge_extra`（`SpeechConfig` 作为 `Settings.speech` 的字段类型，随 `Settings` 一并导出） |
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
| `extra` | String | 该提供商专用的自定义参数（JSON 字符串）；为空时回退到 `ServiceConfig.extra`，**非空时整体覆盖**（不与共享 extra 逐键合并） |

> `extra` 之所以整体覆盖而非逐键合并：同一服务下的提供商可能走**完全不同的协议**（典型是 TTS 同时挂硅基流动 audio/speech 与小米 MiMo chat+audio），逐键合并会把另一协议的残留字段（如 `voice: "FunAudioLLM/...:alex"`）带进来，反而制造隐性冲突。

**`ServiceConfig` — 服务配置（翻译/OCR/TTS 通用）**

| 字段 | 类型 | 说明 |
|------|------|------|
| `model` | String | 默认提供商使用的模型名称 |
| `extra` | String | 共享自定义参数（JSON 字符串），合并到请求体；默认提供商 + 所有未单独填写 `extra` 的提供商都用它 |
| `providers` | `Vec<ExtraProvider>` | 额外的模型提供商列表（默认空数组） |
| `active` | i32 | 当前生效的提供商索引：`-1`（默认值）= 使用全局 base_url+api_key+model；`0+` = `providers[active]` |

`ServiceConfig::with_model_and_extra(model, extra)` 工厂方法：设置 model 和 extra，providers 为空、active=-1。

`ServiceConfig::resolved(default_base_url, default_api_key) -> (String, String, String, String)` 解析当前生效的 `(base_url, api_key, model, extra)`，根据 `active` 字段选择默认或某个额外提供商，并对额外提供商的空字段做全局/共享值回退。三个命令层（translation/ocr/tts）都直接用它的四元组，**不要再单独读 `ServiceConfig.extra`**（那样会绕过 provider 级覆盖）。

**`Settings` — 完整用户配置**

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `base_url` | String | 环境变量 `DEFAULT_BASE_URL`，未设置时 `"https://api.siliconflow.cn"` | 全局共享 API 基础 URL（翻译/OCR/TTS 共用） |
| `api_key` | String | 环境变量 `DEFAULT_API_KEY`，未设置时 `""` | 全局共享 API 密钥（翻译/OCR/TTS 共用） |
| `translation` | ServiceConfig | model=`"tencent/Hunyuan-MT-7B"`, extra=`{"temperature":0.3, "top_p":0.7, "max_tokens":4096, "enable_thinking":false}` | 翻译服务配置 |
| `ocr` | ServiceConfig | model=`"Qwen/Qwen3.5-4B"`, extra=`{"temperature":0.1, "top_p":0.7, "max_tokens":4096, "enable_thinking":false}` | OCR 服务配置 |
| `tts` | ServiceConfig | model=`"FunAudioLLM/CosyVoice2-0.5B"`, extra=`{"voice":"...:alex", "speed":1.0, "response_format":"mp3", "sample_rate":44100}` | TTS 服务配置（默认 provider 走硅基流动 audio/speech；`enable_thinking` 是 chat completions 参数，audio/speech 无此字段，不要加） |
| `hotkeys` | HotkeyConfig | `screenshot="Alt+A"`, `ocr_translate="Alt+S"`, `clipboard_translate="Alt+Q"` | 三个动作的快捷键字符串，使用 `Alt+A`、`Ctrl+Shift+S`、`Cmd+K` 等格式（由 `tauri_plugin_global_shortcut::Shortcut::from_str` 解析） |
| `speech` | SpeechConfig | `auto_read_source=false`, `auto_read_target=false`, `auto_read_max_units=0`, `stream_playback=true` | 朗读行为配置（翻译后自动朗读、自动朗读长度上限、流式边收边播开关） |

**默认 `extra` 里每个值的来历**（据硅基流动 / 小米 MiMo 官方 API 文档，2026-08 核对）

改这些值之前先分清「跟随平台默认」还是「刻意偏离」——前者随上游文档更新，后者动了会改变产品行为：

| 参数 | 平台默认 | 本工具默认 | 说明 |
|------|---------|-----------|------|
| `temperature` | 0.7 | **0.3**（翻译）/ **0.1**（OCR） | 刻意压低换稳定输出，见 [translation.md](translation.md) |
| `top_p` | 0.7 | 0.7 | 跟随平台（曾误填 0.9，2026-08 校正） |
| `max_tokens` | 无默认（受模型上下文窗口约束） | 4096 | 官方示例值；别顶满窗口，留约 10k 给输入 |
| `enable_thinking` | **true** | **false** | 刻意关掉：翻译/OCR 不需要思维链，开着只是多等几秒多花钱。仅对混合推理模型生效，`tencent/Hunyuan-MT-7B` 不在支持列表里会被忽略 |
| TTS `speed` / `gain` | 1.0 / 0.0 | 1.0 / 0.0 | 跟随平台（范围 0.25~4.0 / -10~10） |
| TTS `response_format` | mp3 | mp3 | 跟随平台 |
| TTS `sample_rate` | mp3 与 wav/pcm 均 44100，opus 仅 48000 | 44100 | 跟随平台；与 `response_format` 强耦合，见 [tts.md](tts.md) |
| MiMo `voice` / `format` | mimo_default / wav | 同左 | 见 [tts.md](tts.md)，MiMo 参数不写进默认 extra，按 provider 单独配 |
| MiMo `stream` | false | **true** | 刻意偏离：本工具要边收边播 |

> MiMo 的 chat+audio **没有 `speed` 参数**，`extra.speed` 只对 audio/speech 生效；调 MiMo 语速用 `prefix` / `style`。

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

**`SpeechConfig` — 朗读配置**

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `auto_read_source` | bool | `false` | 翻译完成后自动朗读原文 |
| `auto_read_target` | bool | `false` | 翻译完成后自动朗读译文 |
| `auto_read_max_units` | u32 | `0` | 自动朗读的长度上限（中文按字、其它语种按单词计数）；`0` = 不限制 |
| `stream_playback` | bool | `true` | 边收边播：chat+audio 流式分块实时推给前端播放；关闭则等整段音频合成完再播 |

- 整个结构体用 `#[serde(default)]`，旧版 settings.json（无 `speech` 字段）反序列化时回退到全 false + `stream_playback=true`
- `stream_playback` 用 `#[serde(default = "default_true")]` 保证单独缺字段时默认开启
- 原文/译文可同时开启，前端按「先原文后译文」顺序朗读（`speakSequence`）
- `auto_read_max_units` **只在前端生效**（后端不参与判断）：`hooks/useTranslation.ts` 用 `lib/tts.ts` 的 `countSpeechUnits()` 计数，超限的那一段不入自动朗读队列；手动点喇叭不受限制。默认 `0`（不限制）以保持旧版行为
- `stream_playback` 仅对小米 MiMo（chat+audio）流式路径有意义；audio/speech 协议始终整段播放

- `base_url` 和 `api_key` 字段使用 `#[serde(default)]`，旧版 settings.json（无顶层 base_url/api_key）能正常反序列化并回退到默认值
- `ServiceConfig.providers` 默认空数组、`active` 默认 -1，`ExtraProvider.extra` 默认空串，旧版 settings.json（无这些字段）能正常反序列化并保持原有行为
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
