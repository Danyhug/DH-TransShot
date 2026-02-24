# 配置与状态模块（config/）

## 概述

定义应用全局状态 `AppState` 和用户配置结构体 `Settings`，通过 Mutex 实现线程安全的共享可变状态。

## 文件清单

| 文件 | 职责 |
|------|------|
| `src-tauri/src/config/mod.rs` | 模块声明，公开导出 `AppState`、`Settings`、`ServiceConfig`、`merge_extra` |
| `src-tauri/src/config/settings.rs` | 配置结构体定义、默认值和工具函数 |

## 核心逻辑

### settings.rs

**`ServiceConfig` — 服务配置（翻译/OCR/TTS 通用）**

| 字段 | 类型 | 说明 |
|------|------|------|
| `model` | String | 模型名称 |
| `extra` | String | JSON 字符串，合并到请求体（可覆盖 temperature 等参数） |

`ServiceConfig::with_model(model)` 工厂方法：设置 model，extra 默认为空。

**`Settings` — 完整用户配置**

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `base_url` | String | 环境变量 `DEFAULT_BASE_URL`，未设置时 `"https://api.siliconflow.cn"` | 全局共享 API 基础 URL（翻译/OCR/TTS 共用） |
| `api_key` | String | 环境变量 `DEFAULT_API_KEY`，未设置时 `""` | 全局共享 API 密钥（翻译/OCR/TTS 共用） |
| `translation` | ServiceConfig | model=`"Qwen/Qwen2.5-7B-Instruct"` | 翻译服务配置 |
| `ocr` | ServiceConfig | model=`"PaddlePaddle/PaddleOCR-VL-1.5"` | OCR 服务配置 |
| `tts` | ServiceConfig | model=`""` | TTS 服务配置（预留） |
| `source_language` | String | `"auto"` | 源语言 |
| `target_language` | String | `"zh-CN"` | 目标语言 |
| `hotkey_screenshot` | String | `"Alt+A"` | 区域截图快捷键 |
| `hotkey_region` | String | `"Alt+S"` | 区域翻译快捷键 |

- `base_url` 和 `api_key` 字段使用 `#[serde(default)]`，旧版 settings.json（无顶层 base_url/api_key）能正常反序列化并回退到默认值
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
    pub frozen_screenshot: Mutex<Option<String>>,
    pub frozen_mode: Mutex<String>,
    pub http_client: reqwest::Client,
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `settings` | `Mutex<Settings>` | 用户配置，所有命令共享读写 |
| `frozen_screenshot` | `Mutex<Option<String>>` | 区域选择流程中冻结的全屏截图（base64） |
| `frozen_mode` | `Mutex<String>` | 区域选择模式（`"screenshot"` / `"ocr_translate"`） |
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

每个服务的 model 有独立的硬编码默认值（翻译：`Qwen/Qwen2.5-7B-Instruct`，OCR：`PaddlePaddle/PaddleOCR-VL-1.5`，TTS：空）。

## 依赖关系

- **外部依赖**：`serde`（序列化/反序列化）、`serde_json`（JSON 操作）、`std::sync::Mutex`、`reqwest`（HTTP 客户端）、`dotenvy`（.env 加载）、`log`（日志）
- **被依赖**：
  - `lib.rs` 创建并注册 `AppState`
  - `commands/screenshot.rs` 读写 `frozen_screenshot`、`frozen_mode`
  - `commands/translation.rs` 读取 `settings.base_url`、`settings.api_key`、`settings.translation`、`http_client`
  - `commands/ocr.rs` 读取 `settings.base_url`、`settings.api_key`、`settings.ocr`、`http_client`
  - `commands/settings.rs` 读写 `settings`
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
- `frozen_screenshot` 存储完整 base64 字符串，大截图可能占用大量内存
- **禁止将 API Key 硬编码到源码中**，必须通过 `.env` 文件或用户设置界面配置
- 新增服务类型时，在 `Settings` 中添加对应的 `ServiceConfig` 字段，并更新前端类型和 UI
