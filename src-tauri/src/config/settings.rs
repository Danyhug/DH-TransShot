use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Mutex;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServiceConfig {
    pub model: String,
    pub extra: String,
}

impl ServiceConfig {
    fn with_model(model: &str) -> Self {
        Self {
            model: model.to_string(),
            extra: String::new(),
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
            translation: ServiceConfig::with_model("Qwen/Qwen2.5-7B-Instruct"),
            ocr: ServiceConfig::with_model("PaddlePaddle/PaddleOCR-VL-1.5"),
            tts: ServiceConfig::with_model("IndexTeam/IndexTTS-2"),
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
            http_client: reqwest::Client::new(),
        }
    }
}
