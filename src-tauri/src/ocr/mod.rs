use log::info;

/// Perform OCR using a vision-language model via OpenAI-compatible API.
pub async fn recognize(
    client: &reqwest::Client,
    image_base64: &str,
    _language: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
    extra: &str,
) -> anyhow::Result<String> {
    let url = crate::api_client::chat_completions_url(base_url);
    info!("[OCR] 发送请求到 {}, model={}, image base64 size={}", url, model, image_base64.len());

    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:image/png;base64,{}", image_base64)
                        }
                    },
                    {
                        "type": "text",
                        "text": "请识别图片中的所有文字，只输出识别到的文字内容，不要添加任何解释或格式。"
                    }
                ]
            }
        ],
        "temperature": 0.1
    });

    crate::api_client::send_chat_completion(client, base_url, api_key, extra, request_body, "OCR").await
}
