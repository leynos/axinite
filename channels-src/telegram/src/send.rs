use crate::exports::near::agent::channel::{AgentResponse, Attachment};
use crate::near::agent::channel_host;
use crate::types::{SentMessage, TelegramApiResponse};

/// Text sent to Telegram as a message body.
#[derive(Clone, Copy)]
pub(crate) struct MessageText<'a>(pub(crate) &'a str);

/// Telegram parse mode, for example `Markdown`.
#[derive(Clone, Copy)]
pub(crate) struct ParseMode<'a>(pub(crate) &'a str);

/// A string value that will be percent-encoded for a URL query parameter.
#[derive(Clone, Copy)]
pub(crate) struct QueryParamValue<'a>(pub(crate) &'a str);

/// A Telegram pairing code rendered into a user-facing pairing reply.
#[derive(Clone, Copy)]
pub(crate) struct PairingCode<'a>(pub(crate) &'a str);

/// Multipart/form-data boundary.
#[derive(Clone, Copy)]
struct MultipartBoundary<'a>(&'a str);

/// One multipart/form-data text field.
#[derive(Clone, Copy)]
struct MultipartField<'a> {
    name: &'a str,
    value: &'a str,
}

/// Describes one multipart/form-data file part.
#[derive(Clone, Copy)]
struct MultipartFilePart<'a> {
    field: &'a str,
    filename: &'a str,
    content_type: &'a str,
    data: &'a [u8],
}

/// Parameters for uploading a Telegram file attachment.
#[derive(Clone, Copy)]
struct TelegramUpload<'a> {
    chat_id: i64,
    filename: &'a str,
    mime_type: &'a str,
    data: &'a [u8],
    reply_to_message_id: Option<i64>,
}

/// Parameters that differ between sendPhoto and sendDocument.
struct TelegramMediaEndpoint<'a> {
    /// Multipart form-data field name for the file part.
    field: &'a str,
    /// Telegram Bot API method (e.g. `"sendPhoto"`). Used in the URL and error messages.
    method: &'a str,
    /// Human-readable label for success log messages (e.g. `"photo"`).
    label: &'a str,
}

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
    text: MessageText<'_>,
    reply_to_message_id: Option<i64>,
    parse_mode: Option<ParseMode<'_>>,
) -> Result<i64, SendError> {
    let mut payload = serde_json::json!({
        "chat_id": chat_id,
        "text": text.0,
    });

    if let Some(message_id) = reply_to_message_id {
        payload["reply_to_message_id"] = serde_json::Value::Number(message_id.into());
    }

    if let Some(mode) = parse_mode {
        payload["parse_mode"] = serde_json::Value::String(mode.0.to_string());
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
pub(crate) fn percent_encode(value: QueryParamValue<'_>) -> String {
    let s = value.0;
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
fn write_multipart_field(
    body: &mut Vec<u8>,
    boundary: MultipartBoundary<'_>,
    field: MultipartField<'_>,
) {
    body.extend_from_slice(format!("--{}\r\n", boundary.0).as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{}\"\r\n\r\n",
            field.name
        )
        .as_bytes(),
    );
    body.extend_from_slice(field.value.as_bytes());
    body.extend_from_slice(b"\r\n");
}

/// Write a multipart/form-data file field.
fn write_multipart_file(
    body: &mut Vec<u8>,
    boundary: MultipartBoundary<'_>,
    part: MultipartFilePart<'_>,
) {
    // Sanitize filename: strip quotes, newlines, and non-ASCII to prevent header injection
    let safe_filename: String = part
        .filename
        .chars()
        .filter(|c| *c != '"' && *c != '\r' && *c != '\n' && *c != '\\' && c.is_ascii())
        .collect();
    let safe_filename = if safe_filename.is_empty() {
        "file".to_string()
    } else {
        safe_filename
    };
    body.extend_from_slice(format!("--{}\r\n", boundary.0).as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
            part.field, safe_filename
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", part.content_type).as_bytes());
    body.extend_from_slice(part.data);
    body.extend_from_slice(b"\r\n");
}

/// Core multipart upload shared by `send_photo` and `send_document`.
fn send_multipart_upload(
    upload: TelegramUpload<'_>,
    endpoint: TelegramMediaEndpoint<'_>,
) -> Result<(), String> {
    let boundary = format!("ironclaw-{}", channel_host::now_millis());
    let mut body = Vec::new();

    let chat_id_value = upload.chat_id.to_string();
    write_multipart_field(
        &mut body,
        MultipartBoundary(&boundary),
        MultipartField {
            name: "chat_id",
            value: &chat_id_value,
        },
    );
    if let Some(msg_id) = upload.reply_to_message_id {
        let reply_to_message_id = msg_id.to_string();
        write_multipart_field(
            &mut body,
            MultipartBoundary(&boundary),
            MultipartField {
                name: "reply_to_message_id",
                value: &reply_to_message_id,
            },
        );
    }
    write_multipart_file(
        &mut body,
        MultipartBoundary(&boundary),
        MultipartFilePart {
            field: endpoint.field,
            filename: upload.filename,
            content_type: upload.mime_type,
            data: upload.data,
        },
    );
    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    let headers = serde_json::json!({
        "Content-Type": format!("multipart/form-data; boundary={}", boundary)
    });

    let result = channel_host::http_request(&channel_host::HttpRequestParams {
        method: "POST".to_string(),
        url: format!(
            "https://api.telegram.org/bot{{TELEGRAM_BOT_TOKEN}}/{}",
            endpoint.method
        ),
        headers_json: headers.to_string(),
        body: Some(body),
        timeout_ms: Some(60_000),
    });

    match result {
        Ok(resp) if resp.status == 200 => {
            channel_host::log(
                channel_host::LogLevel::Debug,
                &format!(
                    "Sent {} '{}' to chat {}",
                    endpoint.label, upload.filename, upload.chat_id
                ),
            );
            Ok(())
        }
        Ok(resp) => {
            let body_str = String::from_utf8_lossy(&resp.body);
            Err(format!(
                "{} failed (HTTP {}): {}",
                endpoint.method, resp.status, body_str
            ))
        }
        Err(e) => Err(format!("{} HTTP request failed: {}", endpoint.method, e)),
    }
}

/// Send a photo via the Telegram Bot API (multipart upload).
///
/// Falls back to `send_document()` if the photo exceeds 10 MB.
fn send_photo(upload: TelegramUpload<'_>) -> Result<(), String> {
    if upload.data.len() > MAX_PHOTO_SIZE {
        channel_host::log(
            channel_host::LogLevel::Info,
            &format!(
                "Photo {} exceeds 10MB ({}), sending as document",
                upload.filename,
                upload.data.len()
            ),
        );
        return send_document(upload);
    }
    send_multipart_upload(
        upload,
        TelegramMediaEndpoint {
            field: "photo",
            method: "sendPhoto",
            label: "photo",
        },
    )
}

/// Send a document via the Telegram Bot API (multipart upload).
fn send_document(upload: TelegramUpload<'_>) -> Result<(), String> {
    send_multipart_upload(
        upload,
        TelegramMediaEndpoint {
            field: "document",
            method: "sendDocument",
            label: "document",
        },
    )
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
        MessageText(&response.content),
        reply_to_message_id,
        Some(ParseMode("Markdown")),
    ) {
        Ok(_) => Ok(()),
        Err(SendError::ParseEntities(_)) => {
            send_message(
                chat_id,
                MessageText(&response.content),
                reply_to_message_id,
                None,
            )
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
        send_photo(TelegramUpload {
            chat_id,
            filename: &attachment.filename,
            mime_type: &attachment.mime_type,
            data: &attachment.data,
            reply_to_message_id,
        })
    } else {
        send_document(TelegramUpload {
            chat_id,
            filename: &attachment.filename,
            mime_type: &attachment.mime_type,
            data: &attachment.data,
            reply_to_message_id,
        })
    }
}

/// Send a pairing code message to a chat. Used when an unknown user DMs the bot.
pub(crate) fn send_pairing_reply(chat_id: i64, code: PairingCode<'_>) -> Result<(), String> {
    let message = format!(
        "To pair with this bot, run: `ironclaw pairing approve telegram {}`",
        code.0
    );

    send_message(
        chat_id,
        MessageText(&message),
        None,
        Some(ParseMode("Markdown")),
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}
