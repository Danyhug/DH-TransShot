use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat};
use log::info;
use std::io::Cursor;

const OCR_MAX_LONGEST_EDGE: u32 = 2048;
const OCR_JPEG_QUALITY: u8 = 90;

struct PreparedOcrImage {
    media_type: &'static str,
    base64_data: String,
    width: u32,
    height: u32,
    original_width: u32,
    original_height: u32,
}

fn prepare_ocr_image_from_bytes(bytes: &[u8]) -> anyhow::Result<PreparedOcrImage> {
    let img = image::load_from_memory(bytes)?;
    let (original_width, original_height) = img.dimensions();
    let longest_edge = original_width.max(original_height);

    let processed = if longest_edge > OCR_MAX_LONGEST_EDGE {
        let scale = OCR_MAX_LONGEST_EDGE as f32 / longest_edge as f32;
        let target_width = ((original_width as f32 * scale).round() as u32).max(1);
        let target_height = ((original_height as f32 * scale).round() as u32).max(1);
        let resized = image::imageops::resize(
            &img.to_rgba8(),
            target_width,
            target_height,
            FilterType::Triangle,
        );
        DynamicImage::ImageRgba8(resized)
    } else {
        img
    };

    let (width, height) = processed.dimensions();
    let rgba = processed.to_rgba8();
    let has_transparency = rgba.pixels().any(|pixel| pixel[3] < u8::MAX);

    let mut encoded = Cursor::new(Vec::new());
    let media_type = if has_transparency {
        DynamicImage::ImageRgba8(rgba).write_to(&mut encoded, ImageFormat::Png)?;
        "image/png"
    } else {
        let rgb = DynamicImage::ImageRgba8(rgba).to_rgb8();
        let mut encoder = JpegEncoder::new_with_quality(&mut encoded, OCR_JPEG_QUALITY);
        encoder.encode_image(&DynamicImage::ImageRgb8(rgb))?;
        "image/jpeg"
    };

    Ok(PreparedOcrImage {
        media_type,
        base64_data: base64::engine::general_purpose::STANDARD.encode(encoded.into_inner()),
        width,
        height,
        original_width,
        original_height,
    })
}

/// Perform OCR using a vision-language model via OpenAI-compatible API.
/// Accepts raw image bytes (any format supported by `image::load_from_memory`).
pub async fn recognize(
    client: &reqwest::Client,
    image_bytes: &[u8],
    _language: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
    extra: &str,
) -> anyhow::Result<String> {
    let url = crate::api_client::chat_completions_url(base_url);
    let original_size = image_bytes.len();
    let owned_bytes = image_bytes.to_vec();
    let prepared = tokio::task::spawn_blocking(move || prepare_ocr_image_from_bytes(&owned_bytes))
        .await??;
    info!(
        "[OCR] 发送请求到 {}, model={}, image {}x{} -> {}x{}, media_type={}, size={} -> {}",
        url,
        model,
        prepared.original_width,
        prepared.original_height,
        prepared.width,
        prepared.height,
        prepared.media_type,
        original_size,
        prepared.base64_data.len()
    );

    let request_body = serde_json::json!({
        "model": model,
        "messages": [
            {
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{};base64,{}", prepared.media_type, prepared.base64_data)
                        }
                    },
                    {
                        "type": "text",
                        "text": "请识别图片中的所有文字，只输出纯文本。不要使用Markdown、HTML或其他标记语言，表格内容按行列用纯文本输出。"
                    }
                ]
            }
        ],
        "temperature": 0.1
    });

    crate::api_client::send_chat_completion(client, base_url, api_key, extra, request_body, "OCR").await
}
