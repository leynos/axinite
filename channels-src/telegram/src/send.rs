use crate::exports::near::agent::channel::{AgentResponse, Attachment};
use crate::near::agent::channel_host;
use crate::types::{SentMessage, TelegramApiResponse};

/// Errors from send_message, split so callers can match on parse-entity failures.
pub(crate) enum SendError {
    /// Telegram returned 400 with "can't parse entities" (Markdown issue).
    ParseEntities(String),
    /// Any other failure.
    Other(String),
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SendError::ParseEntities(detail) => write!(f, "parse entities error: {}", detail),
            SendError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

/// Send a message via the Telegram Bot API.
///
/// Returns the sent message_id on success. When `parse_mode` is set and
/// Telegram returns a 400 "can't parse entities" error, returns
/// `SendError::ParseEntities` so the caller can retry without formatting.
pub(crate) fn send_message(
    chat_id: i64,
    text: &str,
    reply_to_message_id: Option<i64>,
    parse_mode: Option<&str>,
) -> Result<i64, SendError> {
    let mut payload = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
    });

    if let Some(message_id) = reply_to_message_id {
        payload["reply_to_message_id"] = serde_json::Value::Number(message_id.into());
    }

    if let Some(mode) = parse_mode {
        payload["parse_mode"] = serde_json::Value::String(mode.to_string());
    }

    let payload_bytes = serde_json::to_vec(&payload)
        .map_err(|e| SendError::Other(format!("Failed to serialize payload: {}", e)))?;

    let headers = serde_json::json!({ "Content-Type": "application/json" });

    let result = channel_host::http_request(&channel_host::HttpRequestParams {
        method: "POST".to_string(),
        url: "https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/sendMessage".to_string(),
        headers_json: headers.to_string(),
        body: Some(payload_bytes.clone()),
        timeout_ms: None,
    });

    match result {
        Ok(http_response) => {
            if http_response.status == 400 {
                let body_str = String::from_utf8_lossy(&http_response.body);
                if body_str.contains("can't parse entities") {
                    return Err(SendError::ParseEntities(body_str.to_string()));
                }
                return Err(SendError::Other(format!(
                    "Telegram API returned 400: {}",
                    body_str
                )));
            }

            if http_response.status != 200 {
                let body_str = String::from_utf8_lossy(&http_response.body);
                return Err(SendError::Other(format!(
                    "Telegram API returned status {}: {}",
                    http_response.status, body_str
                )));
            }

            let api_response: TelegramApiResponse<SentMessage> = serde_json::from_slice(&http_response.body)
                .map_err(|e| SendError::Other(format!("Failed to parse response: {}", e)))?;

            if !api_response.ok {
                return Err(SendError::Other(format!(
                    "Telegram API error: {}",
                    api_response
                        .description
                        .unwrap_or_else(|| "unknown".to_string())
                )));
            }

            Ok(api_response.result.map(|r| r.message_id).unwrap_or(0))
        }
        Err(e) => Err(SendError::Other(format!("HTTP request failed: {}", e))),
    }
}

/// Percent-encode a string for safe use as a URL query parameter value.
pub(crate) fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// Maximum file size for Telegram sendPhoto (10 MB).
const MAX_PHOTO_SIZE: usize = 10 * 1024 * 1024;

/// Write a multipart/form-data text field.
fn write_multipart_field(body: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", name).as_bytes(),
    );
    body.extend_from_slice(value.as_bytes());
    body.extend_from_slice(b"\r\n");
}

/// Write a multipart/form-data file field.
fn write_multipart_file(
    body: &mut Vec<u8>,
    boundary: &str,
    field: &str,
    filename: &str,
    content_type: &str,
    data: &[u8],
) {
    // Sanitize filename: strip quotes, newlines, and non-ASCII to prevent header injection
    let safe_filename: String = filename
        .chars()
        .filter(|c| *c != '"' && *c != '\r' && *c != '\n' && *c != '\\' && c.is_ascii())
        .collect();
    let safe_filename = if safe_filename.is_empty() {
        "file".to_string()
    } else {
        safe_filename
    };
    body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
            field, safe_filename
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", content_type).as_bytes());
    body.extend_from_slice(data);
    body.extend_from_slice(b"\r\n");
}

/// Send a photo via the Telegram Bot API (multipart upload).
///
/// Falls back to `send_document()` if the photo exceeds 10 MB.
fn send_photo(
    chat_id: i64,
    filename: &str,
    mime_type: &str,
    data: &[u8],
    reply_to_message_id: Option<i64>,
) -> Result<(), String> {
    if data.len() > MAX_PHOTO_SIZE {
        channel_host::log(
            channel_host::LogLevel::Info,
            &format!(
                "Photo {} exceeds 10MB ({}), sending as document",
                filename,
                data.len()
            ),
        );
        return send_document(chat_id, filename, mime_type, data, reply_to_message_id);
    }

    let boundary = format!("ironclaw-{}", channel_host::now_millis());
    let mut body = Vec::new();

    write_multipart_field(&mut body, &boundary, "chat_id", &chat_id.to_string());
    if let Some(msg_id) = reply_to_message_id {
        write_multipart_field(&mut body, &boundary, "reply_to_message_id", &msg_id.to_string());
    }
    write_multipart_file(&mut body, &boundary, "photo", filename, mime_type, data);
    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    let headers = serde_json::json!({
        "Content-Type": format!("multipart/form-data; boundary={}", boundary)
    });

    let result = channel_host::http_request(&channel_host::HttpRequestParams {
        method: "POST".to_string(),
        url: "https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/sendPhoto".to_string(),
        headers_json: headers.to_string(),
        body: Some(body.clone()),
        timeout_ms: Some(60_000), // 60s timeout for file uploads
    });

    match result {
        Ok(resp) if resp.status == 200 => {
            channel_host::log(
                channel_host::LogLevel::Debug,
                &format!("Sent photo '{}' to chat {}", filename, chat_id),
            );
            Ok(())
        }
        Ok(resp) => {
            let body_str = String::from_utf8_lossy(&resp.body);
            Err(format!(
                "sendPhoto failed (HTTP {}): {}",
                resp.status, body_str
            ))
        }
        Err(e) => Err(format!("sendPhoto HTTP request failed: {}", e)),
    }
}

/// Send a document via the Telegram Bot API (multipart upload).
fn send_document(
    chat_id: i64,
    filename: &str,
    mime_type: &str,
    data: &[u8],
    reply_to_message_id: Option<i64>,
) -> Result<(), String> {
    let boundary = format!("ironclaw-{}", channel_host::now_millis());
    let mut body = Vec::new();

    write_multipart_field(&mut body, &boundary, "chat_id", &chat_id.to_string());
    if let Some(msg_id) = reply_to_message_id {
        write_multipart_field(&mut body, &boundary, "reply_to_message_id", &msg_id.to_string());
    }
    write_multipart_file(&mut body, &boundary, "document", filename, mime_type, data);
    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    let headers = serde_json::json!({
        "Content-Type": format!("multipart/form-data; boundary={}", boundary)
    });

    let result = channel_host::http_request(&channel_host::HttpRequestParams {
        method: "POST".to_string(),
        url: "https://api.telegram.org/bot{TELEGRAM_BOT_TOKEN}/sendDocument".to_string(),
        headers_json: headers.to_string(),
        body: Some(body.clone()),
        timeout_ms: Some(60_000), // 60s timeout for file uploads
    });

    match result {
        Ok(resp) if resp.status == 200 => {
            channel_host::log(
                channel_host::LogLevel::Debug,
                &format!("Sent document '{}' to chat {}", filename, chat_id),
            );
            Ok(())
        }
        Ok(resp) => {
            let body_str = String::from_utf8_lossy(&resp.body);
            Err(format!(
                "sendDocument failed (HTTP {}): {}",
                resp.status, body_str
            ))
        }
        Err(e) => Err(format!("sendDocument HTTP request failed: {}", e)),
    }
}

/// Image MIME types that Telegram's sendPhoto API supports.
const PHOTO_MIME_TYPES: &[&str] = &["image/jpeg", "image/png", "image/gif", "image/webp"];

/// Send a full agent response (attachments + text) to a chat.
///
/// Shared implementation for both `on_respond` and `on_broadcast`.
pub(crate) fn send_response(
    chat_id: i64,
    response: &AgentResponse,
    reply_to_message_id: Option<i64>,
) -> Result<(), String> {
    // Send attachments first (photos/documents)
    for attachment in &response.attachments {
        send_attachment(chat_id, attachment, reply_to_message_id)?;
    }

    // Skip text if empty and we already sent attachments
    if response.content.is_empty() && !response.attachments.is_empty() {
        return Ok(());
    }

    // Try Markdown, fall back to plain text on parse errors
    match send_message(
        chat_id,
        &response.content,
        reply_to_message_id,
        Some("Markdown"),
    ) {
        Ok(_) => Ok(()),
        Err(SendError::ParseEntities(_)) => {
            send_message(chat_id, &response.content, reply_to_message_id, None)
                .map(|_| ())
                .map_err(|e| format!("Plain-text retry also failed: {}", e))
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Send a single attachment, choosing sendPhoto or sendDocument based on MIME type.
fn send_attachment(
    chat_id: i64,
    attachment: &Attachment,
    reply_to_message_id: Option<i64>,
) -> Result<(), String> {
    if PHOTO_MIME_TYPES.contains(&attachment.mime_type.as_str()) {
        send_photo(
            chat_id,
            &attachment.filename,
            &attachment.mime_type,
            &attachment.data,
            reply_to_message_id,
        )
    } else {
        send_document(
            chat_id,
            &attachment.filename,
            &attachment.mime_type,
            &attachment.data,
            reply_to_message_id,
        )
    }
}

/// Send a pairing code message to a chat. Used when an unknown user DMs the bot.
pub(crate) fn send_pairing_reply(chat_id: i64, code: &str) -> Result<(), String> {
    send_message(
        chat_id,
        &format!(
            "To pair with this bot, run: `ironclaw pairing approve telegram {}`",
            code
        ),
        None,
        Some("Markdown"),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}
