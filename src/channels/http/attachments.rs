//! Webhook attachment validation and decoding.
//!
//! Turns the attachments of a webhook request into `IncomingAttachment`s,
//! enforcing the count and size limits. Rejections carry the HTTP status and
//! message the handler reports.

use axum::http::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

use crate::channels::{AttachmentKind, IncomingAttachment};

/// Maximum size per attachment (5 MB decoded).
const MAX_ATTACHMENT_BYTES: usize = 5 * 1024 * 1024;
/// Maximum total attachment size (10 MB decoded).
const MAX_TOTAL_ATTACHMENT_BYTES: usize = 10 * 1024 * 1024;
/// Maximum number of attachments per request.
const MAX_ATTACHMENTS: usize = 5;

/// A file attachment in a webhook request.
#[derive(Debug, Deserialize)]
pub(super) struct AttachmentData {
    /// MIME type (e.g. "image/png", "application/pdf").
    mime_type: String,
    /// Optional filename.
    #[serde(default)]
    filename: Option<String>,
    /// Base64-encoded file data.
    #[serde(default)]
    data_base64: Option<String>,
    /// URL to fetch the file from (not downloaded server-side for SSRF prevention).
    #[serde(default)]
    url: Option<String>,
}

/// Why an attachment list was refused: the status code and message to report.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct AttachmentRejection {
    pub(super) status: StatusCode,
    pub(super) message: String,
}

impl AttachmentRejection {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

/// Validate the attachment list against the count and size limits and convert
/// each entry to an `IncomingAttachment`.
pub(super) fn decode_attachments(
    attachments: &[AttachmentData],
) -> Result<Vec<IncomingAttachment>, AttachmentRejection> {
    if attachments.len() > MAX_ATTACHMENTS {
        return Err(AttachmentRejection::new(
            StatusCode::BAD_REQUEST,
            format!("Too many attachments (max {})", MAX_ATTACHMENTS),
        ));
    }

    let mut decoded_attachments = Vec::new();
    let mut total_bytes: usize = 0;
    for att in attachments {
        if let Some(decoded) = decode_attachment(att, &mut total_bytes)? {
            decoded_attachments.push(decoded);
        }
    }
    Ok(decoded_attachments)
}

/// Convert one attachment: inline data is decoded and size-checked against the
/// running total; a URL-only attachment records `source_url` and is never
/// downloaded (SSRF prevention); an entry with neither is skipped.
fn decode_attachment(
    att: &AttachmentData,
    total_bytes: &mut usize,
) -> Result<Option<IncomingAttachment>, AttachmentRejection> {
    if let Some(ref b64) = att.data_base64 {
        let data = decode_attachment_bytes(b64, total_bytes)?;
        return Ok(Some(incoming_attachment(att, Some(data), None)));
    }
    Ok(att
        .url
        .as_ref()
        .map(|url| incoming_attachment(att, None, Some(url.clone()))))
}

/// Decode base64 attachment data and charge it to the running total.
fn decode_attachment_bytes(
    b64: &str,
    total_bytes: &mut usize,
) -> Result<Vec<u8>, AttachmentRejection> {
    use base64::Engine;
    let Ok(data) = base64::engine::general_purpose::STANDARD.decode(b64) else {
        return Err(AttachmentRejection::new(
            StatusCode::BAD_REQUEST,
            "Invalid base64 in attachment",
        ));
    };
    if data.len() > MAX_ATTACHMENT_BYTES {
        return Err(AttachmentRejection::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("Attachment too large (max {} bytes)", MAX_ATTACHMENT_BYTES),
        ));
    }
    *total_bytes += data.len();
    if *total_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
        return Err(AttachmentRejection::new(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Total attachment size exceeds limit",
        ));
    }
    Ok(data)
}

/// Build an `IncomingAttachment` from the request entry: inline `data` or a
/// `source_url`, never both.
fn incoming_attachment(
    att: &AttachmentData,
    data: Option<Vec<u8>>,
    source_url: Option<String>,
) -> IncomingAttachment {
    IncomingAttachment {
        id: Uuid::new_v4().to_string(),
        kind: AttachmentKind::from_mime_type(&att.mime_type),
        mime_type: att.mime_type.clone(),
        filename: att.filename.clone(),
        size_bytes: data.as_ref().map(|d| d.len() as u64),
        source_url,
        storage_key: None,
        extracted_text: None,
        data: data.unwrap_or_default(),
        duration_secs: None,
    }
}

#[cfg(test)]
mod tests {
    //! Unit tests for webhook attachment validation and decoding.

    use base64::Engine;

    use super::*;

    fn inline(bytes: &[u8]) -> AttachmentData {
        AttachmentData {
            mime_type: "application/octet-stream".to_string(),
            filename: Some("f.bin".to_string()),
            data_base64: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
            url: None,
        }
    }

    fn url_only(url: &str) -> AttachmentData {
        AttachmentData {
            mime_type: "image/png".to_string(),
            filename: None,
            data_base64: None,
            url: Some(url.to_string()),
        }
    }

    #[test]
    fn inline_data_is_decoded_with_its_size() {
        let decoded = decode_attachments(&[inline(b"abc")]).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].data, b"abc");
        assert_eq!(decoded[0].size_bytes, Some(3));
        assert_eq!(decoded[0].source_url, None);
    }

    #[test]
    fn url_only_records_source_url_without_data() {
        let decoded = decode_attachments(&[url_only("https://example.com/a.png")]).unwrap();
        assert_eq!(decoded.len(), 1);
        assert!(decoded[0].data.is_empty());
        assert_eq!(decoded[0].size_bytes, None);
        assert_eq!(
            decoded[0].source_url.as_deref(),
            Some("https://example.com/a.png")
        );
    }

    #[test]
    fn entry_with_neither_data_nor_url_is_skipped() {
        let empty = AttachmentData {
            mime_type: "text/plain".to_string(),
            filename: None,
            data_base64: None,
            url: None,
        };
        assert!(decode_attachments(&[empty]).unwrap().is_empty());
    }

    #[test]
    fn inline_data_wins_over_url() {
        let mut both = inline(b"xy");
        both.url = Some("https://example.com/x".to_string());
        let decoded = decode_attachments(&[both]).unwrap();
        assert_eq!(decoded[0].source_url, None);
        assert_eq!(decoded[0].data, b"xy");
    }

    #[test]
    fn too_many_attachments_is_a_bad_request() {
        let list: Vec<_> = (0..=MAX_ATTACHMENTS).map(|_| inline(b"a")).collect();
        let err = decode_attachments(&list).unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert_eq!(err.message, "Too many attachments (max 5)");
    }

    #[test]
    fn invalid_base64_is_a_bad_request() {
        let mut bad = inline(b"a");
        bad.data_base64 = Some("!!!not base64!!!".to_string());
        let err = decode_attachments(&[bad]).unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert_eq!(err.message, "Invalid base64 in attachment");
    }

    #[test]
    fn oversized_attachment_is_rejected() {
        let big = inline(&vec![0u8; MAX_ATTACHMENT_BYTES + 1]);
        let err = decode_attachments(&[big]).unwrap_err();
        assert_eq!(err.status, StatusCode::PAYLOAD_TOO_LARGE);
        assert!(err.message.starts_with("Attachment too large"));
    }

    #[test]
    fn running_total_across_attachments_is_enforced() {
        let each = vec![0u8; MAX_ATTACHMENT_BYTES];
        let err = decode_attachments(&[inline(&each), inline(&each), inline(b"x")]).unwrap_err();
        assert_eq!(err.status, StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(err.message, "Total attachment size exceeds limit");
    }
}
