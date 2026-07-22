use crate::config::merge_extra;
use log::{error, warn};
use reqwest::Client;
use serde::Deserialize;

/// Shared Chat Completions response structures (OpenAI-compatible).
#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Deserialize)]
struct ChatMessage {
    content: String,
}

/// 把 base_url 与 OpenAI 风格端点路径拼接，按用户在设置里填写的形态自适应：
///
/// 0. 以 `#` 结尾 → 去掉标记后原样使用（raw 模式，支持任意非标准/自定义路径）
/// 1. 已含完整端点（如 `.../chat/completions`）→ 原样使用
/// 2. 末尾是版本段（`v1`/`v4`/`v1beta`...）→ 追加 `/endpoint`（保留用户自己的版本号）
/// 3. 否则当作根地址 → 追加 `/v1/endpoint`
///
/// `endpoint` 是版本段之后的相对路径，如 `"chat/completions"` 或 `"audio/speech"`。
pub fn build_endpoint_url(base_url: &str, endpoint: &str) -> String {
    let trimmed = base_url.trim();

    // 0) 显式 raw 模式：末尾 '#' → 去掉标记后原样使用，忽略端点自动补全
    if let Some(raw) = trimmed.strip_suffix('#') {
        return raw.to_string();
    }

    let base = trimmed.trim_end_matches('/');
    let endpoint = endpoint.trim_matches('/');

    // 1) 用户已填完整端点
    if base.ends_with(&format!("/{endpoint}")) {
        return base.to_string();
    }

    // 2) 末尾是版本段 → 直接接端点，保留原版本号
    let last_seg = base.rsplit('/').next().unwrap_or("");
    if is_version_segment(last_seg) {
        return format!("{base}/{endpoint}");
    }

    // 3) 兜底：当作根地址
    format!("{base}/v1/{endpoint}")
}

/// 判断是否为版本段（`v1`/`v4`/`v1beta`...）：以 `v` 开头且紧跟数字。
fn is_version_segment(seg: &str) -> bool {
    let mut chars = seg.chars();
    chars.next() == Some('v') && chars.next().is_some_and(|c| c.is_ascii_digit())
}

/// Build the full Chat Completions endpoint URL from a base_url.
pub fn chat_completions_url(base_url: &str) -> String {
    build_endpoint_url(base_url, "chat/completions")
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
