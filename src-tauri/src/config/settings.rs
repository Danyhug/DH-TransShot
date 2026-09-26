use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ExtraProvider {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    /// 该提供商专用的自定义参数（JSON 字符串）。
    /// 为空时回退到 `ServiceConfig.extra`；非空时**整体覆盖**（不与共享 extra 逐键合并），
    /// 便于让不同协议的提供商（如 audio/speech 与小米 chat+audio）各带各的参数互不干扰。
    #[serde(default)]
    pub extra: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServiceConfig {
    pub model: String,
    /// 共享自定义参数：默认提供商，以及所有未单独填写 `extra` 的额外提供商都用它
    pub extra: String,
    #[serde(default)]
    pub providers: Vec<ExtraProvider>,
    #[serde(default = "default_active_provider")]
    pub active: i32,
}

fn default_active_provider() -> i32 {
    -1
}

impl ServiceConfig {
    fn with_model_and_extra(model: &str, extra: &str) -> Self {
        Self {
            model: model.to_string(),
            extra: extra.to_string(),
            providers: Vec::new(),
            active: -1,
        }
    }

    /// Resolve the active (base_url, api_key, model, extra) based on `active` index.
    /// `active < 0` or out-of-range falls back to the default (global creds + self.model/extra).
    /// For extra providers, empty `base_url`/`api_key`/`model`/`extra` fall back to the shared ones.
    pub fn resolved(
        &self,
        default_base_url: &str,
        default_api_key: &str,
    ) -> (String, String, String, String) {
        let fallback = || {
            (
                default_base_url.to_string(),
                default_api_key.to_string(),
                self.model.clone(),
                self.extra.clone(),
            )
        };
        if self.active < 0 {
            return fallback();
        }
        let idx = self.active as usize;
        match self.providers.get(idx) {
            Some(p) => {
                let base = if p.base_url.trim().is_empty() {
                    default_base_url.to_string()
                } else {
                    p.base_url.clone()
                };
                let key = if p.api_key.trim().is_empty() {
                    default_api_key.to_string()
                } else {
                    p.api_key.clone()
                };
                let model = if p.model.trim().is_empty() {
                    self.model.clone()
                } else {
                    p.model.clone()
                };
                let extra = if p.extra.trim().is_empty() {
                    self.extra.clone()
                } else {
                    p.extra.clone()
                };
                (base, key, model, extra)
            }
            None => fallback(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HotkeyConfig {
    #[serde(default = "default_hotkey_screenshot")]
    pub screenshot: String,
    #[serde(default = "default_hotkey_ocr_translate")]
    pub ocr_translate: String,
    #[serde(default = "default_hotkey_clipboard_translate")]
    pub clipboard_translate: String,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            screenshot: default_hotkey_screenshot(),
            ocr_translate: default_hotkey_ocr_translate(),
            clipboard_translate: default_hotkey_clipboard_translate(),
        }
    }
}

fn default_hotkey_screenshot() -> String {
    "Alt+A".to_string()
}

fn default_hotkey_ocr_translate() -> String {
    "Alt+S".to_string()
}

fn default_hotkey_clipboard_translate() -> String {
    "Alt+Q".to_string()
}

/// 朗读行为配置（翻译完成后的自动朗读 / 流式播放开关）。
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SpeechConfig {
    /// 翻译完成后自动朗读原文
    #[serde(default)]
    pub auto_read_source: bool,
    /// 翻译完成后自动朗读译文
    #[serde(default)]
    pub auto_read_target: bool,
    /// 自动朗读的长度上限（中文按字、西文按单词计数）；`0` 表示不限制。
    /// 只约束自动朗读，手动点击喇叭始终朗读。
    #[serde(default)]
    pub auto_read_max_units: u32,
    /// 边收边播：chat+audio 流式分块实时推给前端播放（关闭则等整段音频合成完再播）
    #[serde(default = "default_true")]
    pub stream_playback: bool,
}

fn default_true() -> bool {
    true
}

impl Default for SpeechConfig {
    fn default() -> Self {
        Self {
            auto_read_source: false,
            auto_read_target: false,
            auto_read_max_units: 0,
            stream_playback: true,
        }
    }
}

/// 翻译提示词偏好：注入到翻译 system prompt 里，由 `translation::prompt` 负责拼装。
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct TranslationPromptConfig {
    /// 解释缩写与标识符：原文是单独的缩写时输出按可能性排序的「全称 + 含义」候选；
    /// 原文是单独的标识符/路径（如 `stores/settingsStore`）时按词段译出含义；
    /// 句子里的缩写按上下文译出含义
    pub expand_abbreviations: bool,
    /// 行业偏向（`translation::prompt::DOMAINS` 里的 key，可多选）；空 = 通用
    pub domains: Vec<String>,
    /// 用户自定义的附加翻译指令，原样追加到 system prompt
    pub custom_prompt: String,
    /// 编辑过的内置翻译规则；空 = 使用 `translation::prompt::DEFAULT_RULES`。
    /// 存空串而不是默认文本，内置规则后续升级时未改过的用户能自动跟上
    pub base_prompt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default = "default_api_key")]
    pub api_key: String,
    pub translation: ServiceConfig,
    pub ocr: ServiceConfig,
    pub tts: ServiceConfig,
    #[serde(default)]
    pub hotkeys: HotkeyConfig,
    #[serde(default)]
    pub speech: SpeechConfig,
    #[serde(default)]
    pub translation_prompt: TranslationPromptConfig,
}

fn default_base_url() -> String {
    std::env::var("DEFAULT_BASE_URL").unwrap_or_else(|_| "https://api.siliconflow.cn".to_string())
}

fn default_api_key() -> String {
    std::env::var("DEFAULT_API_KEY").unwrap_or_default()
}

/// 首次启动（或配置文件缺字段）时写入的默认配置。
///
/// extra 里的值分两类，改动前先分清楚（据硅基流动官方 API 文档，2026-08 核对）：
/// - **跟随平台默认**：`top_p`=0.7、`max_tokens`=4096（接口本身无默认，取决于模型上下文窗口）、
///   TTS 的 `speed`=1.0 / `response_format`=mp3 / `sample_rate`=44100（mp3 与 wav/pcm 同为 44100，
///   opus 只有 48000）
/// - **刻意偏离平台默认**：`temperature` 平台默认 0.7，这里压到 0.3（翻译）/ 0.1（OCR）换取稳定；
///   `enable_thinking` 平台默认 **true**，这里显式关掉——翻译/OCR 不需要思维链，开着只是白等白花钱
///
/// 注意 `enable_thinking` 只对部分模型有效（`tencent/Hunyuan-MT-7B` 不在支持列表里，会被忽略），
/// 留着是为了换成 Qwen3.5 / GLM 这类混合推理模型时不会突然多出一段思考。
impl Default for Settings {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key: default_api_key(),
            translation: ServiceConfig::with_model_and_extra(
                "tencent/Hunyuan-MT-7B",
                r#"{
  "temperature": 0.3,
  "top_p": 0.7,
  "max_tokens": 4096,
  "enable_thinking": false
}"#,
            ),
            ocr: ServiceConfig::with_model_and_extra(
                "Qwen/Qwen3.5-4B",
                r#"{
  "temperature": 0.1,
  "top_p": 0.7,
  "max_tokens": 4096,
  "enable_thinking": false
}"#,
            ),
            tts: ServiceConfig::with_model_and_extra(
                "FunAudioLLM/CosyVoice2-0.5B",
                r#"{
  "voice": "FunAudioLLM/CosyVoice2-0.5B:alex",
  "speed": 1.0,
  "response_format": "mp3",
  "sample_rate": 44100
}"#,
            ),
            hotkeys: HotkeyConfig::default(),
            speech: SpeechConfig::default(),
            translation_prompt: TranslationPromptConfig::default(),
        }
    }
}

/// Merge a JSON string into an existing request body.
/// Keys in `extra` override existing keys in `body`.
/// Logs a warning with `[tag]` prefix if `extra` is non-empty but invalid JSON.
pub fn merge_extra(body: &mut Value, extra: &str, tag: &str) {
    let trimmed = extra.trim();
    if trimmed.is_empty() {
        return;
    }
    match serde_json::from_str::<Value>(trimmed) {
        Ok(Value::Object(map)) => {
            if let Value::Object(ref mut target) = body {
                for (k, v) in map {
                    target.insert(k, v);
                }
            }
        }
        Ok(_) => {
            warn!("[{}] extra 不是 JSON 对象，已忽略", tag);
        }
        Err(e) => {
            warn!("[{}] extra JSON 解析失败: {}", tag, e);
        }
    }
}

/// Monitor information for multi-monitor screenshot support.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MonitorInfo {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub frozen_screenshots: Mutex<Vec<String>>,
    pub frozen_mode: Mutex<String>,
    pub frozen_window_rects: Mutex<serde_json::Value>,
    pub frozen_monitors: Mutex<Vec<MonitorInfo>>,
    pub tts_cache: Mutex<TtsCache>,
    pub http_client: reqwest::Client,
    /// 本地音频输出（朗读播放）。播放放在 Rust 侧而不是 WebView，原因见 `crate::audio`
    pub audio: crate::audio::AudioOutput,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: Mutex::new(Settings::default()),
            frozen_screenshots: Mutex::new(Vec::new()),
            frozen_mode: Mutex::new(String::new()),
            frozen_window_rects: Mutex::new(serde_json::Value::Array(vec![])),
            frozen_monitors: Mutex::new(Vec::new()),
            tts_cache: Mutex::new(TtsCache::default()),
            http_client: reqwest::Client::new(),
            audio: crate::audio::AudioOutput::default(),
        }
    }
}

const TTS_CACHE_MAX_ENTRIES: usize = 64;
/// 缓存总字节上限（base64 字符数累加）。只按条数限制是不够的：一分钟语音拼出来的
/// WAV base64 就有 ~3.8MB，64 条塞满能占几百 MB 常驻内存。
const TTS_CACHE_MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Default)]
pub struct TtsCache {
    entries: HashMap<String, String>,
    order: VecDeque<String>,
    /// 当前占用字节数（各 value 的 len 之和）
    bytes: usize,
}

impl TtsCache {
    pub fn get(&self, key: &str) -> Option<String> {
        self.entries.get(key).cloned()
    }

    pub fn insert(&mut self, key: String, value: String) {
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.len();
            self.order.retain(|existing| existing != &key);
        }
        self.bytes += value.len();
        self.entries.insert(key.clone(), value);
        self.order.push_back(key);

        // 条数和字节数两个上限都要满足；但至少保留刚写入的这条，
        // 否则单条就超预算时会被立刻淘汰，缓存永远空转
        while self.order.len() > TTS_CACHE_MAX_ENTRIES
            || (self.bytes > TTS_CACHE_MAX_BYTES && self.order.len() > 1)
        {
            let Some(oldest_key) = self.order.pop_front() else {
                break;
            };
            if let Some(old) = self.entries.remove(&oldest_key) {
                self.bytes -= old.len();
            }
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_evicts_by_entry_count() {
        let mut cache = TtsCache::default();
        for i in 0..TTS_CACHE_MAX_ENTRIES + 3 {
            cache.insert(format!("k{i}"), "x".to_string());
        }
        assert_eq!(cache.entries.len(), TTS_CACHE_MAX_ENTRIES);
        assert_eq!(cache.get("k0"), None);
        assert_eq!(
            cache.get(&format!("k{}", TTS_CACHE_MAX_ENTRIES + 2)),
            Some("x".to_string())
        );
    }

    /// 只按条数限制时，几条长音频就能吃掉几百 MB
    #[test]
    fn cache_evicts_by_total_bytes() {
        let mut cache = TtsCache::default();
        let big = "x".repeat(TTS_CACHE_MAX_BYTES / 2 + 1);
        cache.insert("a".into(), big.clone());
        cache.insert("b".into(), big.clone());
        assert_eq!(cache.get("a"), None, "超预算时最旧的一条应被淘汰");
        assert!(cache.get("b").is_some());
        assert!(cache.bytes <= TTS_CACHE_MAX_BYTES);
    }

    /// 单条就超预算时也不能把自己淘汰掉，否则缓存永远是空的
    #[test]
    fn cache_keeps_single_oversized_entry() {
        let mut cache = TtsCache::default();
        cache.insert("a".into(), "x".repeat(TTS_CACHE_MAX_BYTES + 1));
        assert!(cache.get("a").is_some());
    }

    /// 覆盖同一个键时字节数要正确回退，否则会越算越大、把缓存挤空
    #[test]
    fn cache_overwrite_tracks_bytes() {
        let mut cache = TtsCache::default();
        cache.insert("a".into(), "xxxxx".into());
        cache.insert("a".into(), "y".into());
        assert_eq!(cache.bytes, 1);
        assert_eq!(cache.order.len(), 1);
        assert_eq!(cache.get("a"), Some("y".to_string()));
        cache.clear();
        assert_eq!(cache.bytes, 0);
    }
}
