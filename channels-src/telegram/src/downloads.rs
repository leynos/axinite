use crate::near::agent::channel_host::{self, InboundAttachment};
use crate::send::{QueryParamValue, percent_encode};
use crate::types::{TelegramApiResponse, TelegramFile};

/// Maximum file size to download (20 MB). Files larger than this are discarded
/// to avoid excessive memory use and slow downloads in the WASM runtime.
pub(crate) const MAX_DOWNLOAD_SIZE_BYTES: u64 = 20 * 1024 * 1024;

/// Validate `file_id` and call the Telegram `getFile` API to resolve the
/// server-side `file_path`.
fn resolve_file_path(file_id: &str) -> Result<String, String> {
    if file_id.contains('{') || file_id.contains('}') {
        return Err("invalid file_id: contains forbidden characters".to_string());
    }

    let get_file_url = format!(
        "https://api.telegram.org/bot{{TELEGRAM_BOT_TOKEN}}/getFile?file_id={}",
        percent_encode(QueryParamValue(file_id))
    );

    let headers = serde_json::json!({});
    let response = channel_host::http_request(&channel_host::HttpRequestParams {
        method: "GET".to_string(),
        url: get_file_url,
        headers_json: headers.to_string(),
        body: None,
        timeout_ms: None,
    })
    .map_err(|e| format!("getFile request failed: {}", e))?;

    if response.status != 200 {
        let body_str = String::from_utf8_lossy(&response.body);
        return Err(format!(
            "getFile returned {}: {}",
            response.status, body_str
        ));
    }

    let api_response: TelegramApiResponse<TelegramFile> = serde_json::from_slice(&response.body)
        .map_err(|e| format!("Failed to parse getFile response: {}", e))?;

    if !api_response.ok {
        return Err(format!(
            "getFile API error: {}",
            api_response
                .description
                .unwrap_or_else(|| "unknown".to_string())
        ));
    }

    let file = api_response
        .result
        .ok_or_else(|| "getFile returned no result".to_string())?;

    file.file_path
        .ok_or_else(|| "getFile returned no file_path".to_string())
}

/// Validate `file_path`, fetch the raw bytes, and enforce the size limit.
fn fetch_file_bytes(file_path: &str) -> Result<Vec<u8>, String> {
    if file_path.contains('{') || file_path.contains('}') {
        return Err("invalid file_path: contains forbidden characters".to_string());
    }

    let download_url = format!(
        "https://api.telegram.org/file/bot{{TELEGRAM_BOT_TOKEN}}/{}",
        file_path
    );

    let headers = serde_json::json!({});
    let response = channel_host::http_request(&channel_host::HttpRequestParams {
        method: "GET".to_string(),
        url: download_url,
        headers_json: headers.to_string(),
        body: None,
        timeout_ms: None,
    })
    .map_err(|e| format!("File download failed: {}", e))?;

    if response.status != 200 {
        return Err(format!("File download returned status {}", response.status));
    }

    if response.body.len() as u64 > MAX_DOWNLOAD_SIZE_BYTES {
        return Err(format!(
            "Downloaded file exceeds {} MB limit ({} bytes)",
            MAX_DOWNLOAD_SIZE_BYTES / (1024 * 1024),
            response.body.len()
        ));
    }

    Ok(response.body)
}

fn download_telegram_file(file_id: &str) -> Result<Vec<u8>, String> {
    let file_path = resolve_file_path(file_id)?;
    fetch_file_bytes(&file_path)
}

fn is_voice_attachment(att: &InboundAttachment) -> bool {
    // Voice attachments have a generated filename like "voice_<id>.ogg"
    att.filename
        .as_ref()
        .is_some_and(|f| f.starts_with("voice_"))
}

fn is_image_attachment(att: &InboundAttachment) -> bool {
    att.mime_type.starts_with("image/")
}

fn is_audio_attachment(att: &InboundAttachment) -> bool {
    att.mime_type.starts_with("audio/")
}

fn is_video_attachment(att: &InboundAttachment) -> bool {
    att.mime_type.starts_with("video/")
}

fn is_media_attachment(att: &InboundAttachment) -> bool {
    is_image_attachment(att) || is_audio_attachment(att) || is_video_attachment(att)
}

fn store_downloaded_attachment(att: &InboundAttachment, bytes: &[u8], data_kind: &str) {
    if let Err(e) = channel_host::store_attachment_data(&att.id, bytes) {
        channel_host::log(
            channel_host::LogLevel::Error,
            &format!("Failed to store {} data: {}", data_kind, e),
        );
    }
}

fn download_and_store_attachment(att: &InboundAttachment, data_kind: &str) {
    match download_telegram_file(&att.id) {
        Ok(bytes) => {
            channel_host::log(
                channel_host::LogLevel::Info,
                &format!("Downloaded {} file: {} bytes", data_kind, bytes.len()),
            );
            store_downloaded_attachment(att, &bytes, data_kind);
        }
        Err(e) => {
            channel_host::log(
                channel_host::LogLevel::Error,
                &format!("Failed to download {} file: {}", data_kind, e),
            );
        }
    }
}

#[cfg(test)]
fn download_and_store_document_attachment(att: &mut InboundAttachment) {
    let _ = att;
}

#[cfg(not(test))]
fn download_and_store_document_attachment(att: &mut InboundAttachment) {
    match download_telegram_file(&att.id) {
        Ok(bytes) => {
            channel_host::log(
                channel_host::LogLevel::Info,
                &format!(
                    "Downloaded document file: {} bytes, mime={}",
                    bytes.len(),
                    att.mime_type
                ),
            );
            if let Err(e) = channel_host::store_attachment_data(&att.id, &bytes) {
                channel_host::log(
                    channel_host::LogLevel::Error,
                    &format!("Failed to store document data: {}", e),
                );
            }
        }
        Err(e) => {
            channel_host::log(
                channel_host::LogLevel::Error,
                &format!("Failed to download document file: {}", e),
            );
            let name = att.filename.as_deref().unwrap_or("document");
            att.extracted_text = Some(format!(
                "[Failed to download '{name}': {e}. \
                 The file may be too large or unavailable. Please try a smaller file.]"
            ));
        }
    }
}

fn download_and_store_matching_attachments(
    attachments: &[InboundAttachment],
    data_kind: &str,
    predicate: impl Fn(&InboundAttachment) -> bool,
) {
    for att in attachments.iter().filter(|att| predicate(att)) {
        download_and_store_attachment(att, data_kind);
    }
}

/// Download voice file bytes and store them via the host for transcription.
///
/// Separated from `extract_attachments` so that function stays pure (no host
/// calls) and remains testable in native unit tests.
pub(crate) fn download_and_store_voice(attachments: &[InboundAttachment]) {
    download_and_store_matching_attachments(attachments, "voice", is_voice_attachment);
}

/// Download image file bytes and store them via the host for the vision pipeline.
///
/// Separated from `extract_attachments` so that function stays pure (no host
/// calls) and remains testable in native unit tests.
pub(crate) fn download_and_store_images(attachments: &[InboundAttachment]) {
    download_and_store_matching_attachments(attachments, "image", is_image_attachment);
}

/// Returns true if the attachment should be downloaded for document text extraction.
///
/// Excludes voice (handled by transcription), image (vision pipeline),
/// audio (transcription), and video attachments.
pub(crate) fn is_downloadable_document(att: &InboundAttachment) -> bool {
    if is_voice_attachment(att) {
        return false;
    }

    if is_media_attachment(att) {
        return false;
    }

    true
}

/// Download document file bytes and store them via the host for text extraction.
///
/// Downloads any attachment that isn't voice or image so the host-side
/// `DocumentExtractionMiddleware` can extract text from PDFs, Office docs, etc.
///
/// On failure, sets `extracted_text` to an error message so the user gets feedback.
pub(crate) fn download_and_store_documents(attachments: &mut [InboundAttachment]) {
    for att in attachments
        .iter_mut()
        .filter(|att| is_downloadable_document(att))
    {
        download_and_store_document_attachment(att);
    }
}
