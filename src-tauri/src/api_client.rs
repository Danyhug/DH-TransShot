use crate::config::merge_extra;
use log::{error, warn};
use reqwest::Client;
use serde::Deserialize;

/// Shared Chat Completions response structures (OpenAI-compatible).
#[derive(Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
pub struct ChatChoice {
    pub message: ChatMessage,
}

#[derive(Deserialize)]
pub struct ChatMessage {
    pub content: String,
}

/// Build the full Chat Completions endpoint URL from a base_url.
pub fn chat_completions_url(base_url: &str) -> String {
    format!("{}/v1/chat/completions", base_url.trim_end_matches('/'))
}

/// Send a Chat Completions request and return the first choice's content.
///
/// Handles:
/// - `extra` JSON merging into the request body
/// - Bearer auth (skipped if `api_key` is empty)
/// - HTTP error status → anyhow error with body
/// - Extracting the first choice's message content
pub async fn send_chat_completion(
    client: &Client,
    base_url: &str,
    api_key: &str,
    extra: &str,
    mut request_body: serde_json::Value,
    tag: &str,
) -> anyhow::Result<String> {
    if base_url.trim().is_empty() {
        anyhow::bail!("{} 未配置 API 地址，请在设置中填写 base_url", tag);
    }

    let url = chat_completions_url(base_url);

    merge_extra(&mut request_body, extra, tag);

    let mut req = client.post(&url).json(&request_body);
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    } else {
        warn!("[{}] API Key 为空", tag);
    }

    let response = req.send().await?;
    let status = response.status();

    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        error!("[{}] API 错误 ({}): {}", tag, status, body);
        anyhow::bail!("{} API error ({}): {}", tag, status, body);
    }

    let chat_response: ChatResponse = response.json().await?;
    chat_response
        .choices
        .first()
        .map(|c| c.message.content.trim().to_string())
        .ok_or_else(|| anyhow::anyhow!("No result in {} response", tag))
}
