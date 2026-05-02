use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServiceConfig {
    pub model: String,
    pub extra: String,
}

impl ServiceConfig {
    fn with_model_and_extra(model: &str, extra: &str) -> Self {
        Self {
            model: model.to_string(),
            extra: extra.to_string(),
        }
    }
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
    pub source_language: String,
    pub target_language: String,
    pub hotkey_screenshot: String,
    pub hotkey_region: String,
}

fn default_base_url() -> String {
    std::env::var("DEFAULT_BASE_URL")
        .unwrap_or_else(|_| "https://api.siliconflow.cn".to_string())
}

fn default_api_key() -> String {
    std::env::var("DEFAULT_API_KEY").unwrap_or_default()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key: default_api_key(),
            translation: ServiceConfig::with_model_and_extra(
                "tencent/Hunyuan-MT-7B",
                r#"{
  "temperature": 0.3,
  "top_p": 0.9,
  "max_tokens": 4096
}"#,
            ),
            ocr: ServiceConfig::with_model_and_extra(
                "PaddlePaddle/PaddleOCR-VL-1.5",
                r#"{
  "temperature": 0.1,
  "top_p": 0.9,
  "max_tokens": 4096
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
            source_language: "auto".to_string(),
            target_language: "zh-CN".to_string(),
            hotkey_screenshot: "Alt+A".to_string(),
            hotkey_region: "Alt+S".to_string(),
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
        }
    }
}

const TTS_CACHE_MAX_ENTRIES: usize = 64;

#[derive(Debug, Default)]
pub struct TtsCache {
    entries: HashMap<String, String>,
    order: VecDeque<String>,
}

impl TtsCache {
    pub fn get(&self, key: &str) -> Option<String> {
        self.entries.get(key).cloned()
    }

    pub fn insert(&mut self, key: String, value: String) {
        if self.entries.contains_key(&key) {
            self.order.retain(|existing| existing != &key);
        }
        self.entries.insert(key.clone(), value);
        self.order.push_back(key);

        while self.order.len() > TTS_CACHE_MAX_ENTRIES {
            if let Some(oldest_key) = self.order.pop_front() {
                self.entries.remove(&oldest_key);
            }
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}
