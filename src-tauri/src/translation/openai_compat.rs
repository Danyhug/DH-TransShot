use log::info;
use reqwest::Client;

pub struct OpenAiCompatProvider {
    client: Client,
}

impl OpenAiCompatProvider {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn translate(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
        base_url: &str,
        api_key: &str,
        model: &str,
        extra: &str,
    ) -> anyhow::Result<String> {
        let source_display = if source_lang == "auto" {
            "the detected language".to_string()
        } else {
            source_lang.to_string()
        };

        let system_prompt = format!(
            "You are a translator. Translate the following text from {} to {}. \
             Output ONLY the translated text, nothing else. \
             Do not add explanations, notes, or any extra content.",
            source_display, target_lang
        );

        let url = crate::api_client::chat_completions_url(base_url);
        info!("[Translation] 发送请求到 {}, model={}", url, model);

        let request_body = serde_json::json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": text }
            ],
            "temperature": 0.3
        });

        crate::api_client::send_chat_completion(&self.client, base_url, api_key, extra, request_body, "Translation").await
    }
}
