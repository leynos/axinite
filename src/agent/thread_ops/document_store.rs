//! Extracted-document storage helpers for thread operations.

use std::sync::Arc;

use crate::channels::{IncomingAttachment, IncomingMessage};

/// Replace characters that are unsafe in file-system paths with an underscore.
fn sanitise_filename_char(c: char) -> char {
    if matches!(
        c,
        '/' | '\\' | '\0' | '\u{2215}' | '\u{2216}' | '\u{29F5}' | '\u{FF0F}' | '\u{FF3C}'
    ) {
        '_'
    } else {
        c
    }
}

/// Returns `true` when extracted text is usable — i.e. not an error sentinel.
fn is_usable_extracted_text(t: &str) -> bool {
    !t.starts_with("[Failed") && !t.starts_with("[Error") && !t.starts_with("[Unsupported")
}

fn get_valid_document_text(attachment: &IncomingAttachment) -> Option<&str> {
    match &attachment.extracted_text {
        Some(t) if is_usable_extracted_text(t) => Some(t),
        _ => None,
    }
}

fn sanitise_filename(raw_name: &str) -> String {
    let filename: String = raw_name.chars().map(sanitise_filename_char).collect();
    let filename = filename.replace("..", "__");
    let filename = filename.trim_start_matches('.');
    if filename.is_empty() {
        "unnamed_document".to_string()
    } else {
        filename.to_string()
    }
}

fn build_document_path(index: usize, attachment: &IncomingAttachment, date: &str) -> String {
    let raw_name = attachment.filename.as_deref().unwrap_or("unnamed_document");
    let filename = sanitise_filename(raw_name);
    let raw_id = if attachment.id.is_empty() {
        "unnamed_id"
    } else {
        attachment.id.as_str()
    };
    let sanitized_id = sanitise_filename(raw_id);

    format!("documents/{date}/{index}-{sanitized_id}-{filename}")
}

async fn write_document_to_workspace(
    workspace: &Arc<crate::workspace::Workspace>,
    path: &str,
    content: &str,
    text_len: usize,
) {
    match workspace.write(path, content).await {
        Ok(_) => {
            tracing::info!(
                path = %path,
                text_len,
                "Stored extracted document in workspace memory"
            );
        }
        Err(e) => {
            tracing::warn!(
                path = %path,
                error = %e,
                "Failed to store extracted document in workspace"
            );
        }
    }
}

/// Store extracted document text in workspace memory for future search/recall.
pub(super) async fn store_extracted_documents(
    workspace: &Arc<crate::workspace::Workspace>,
    message: &IncomingMessage,
) {
    let date = chrono::Utc::now().format("%Y-%m-%d").to_string();

    for (index, attachment) in message.attachments.iter().enumerate() {
        if attachment.kind != crate::channels::AttachmentKind::Document {
            continue;
        }
        let Some(text) = get_valid_document_text(attachment) else {
            continue;
        };

        let filename =
            sanitise_filename(attachment.filename.as_deref().unwrap_or("unnamed_document"));
        let path = build_document_path(index, attachment, &date);

        let header = format!(
            "# {filename}\n\n\
             > Uploaded by **{}** via **{}** on {date}\n\
             > MIME: {} | Size: {} bytes\n\n---\n\n",
            "uploader",
            message.channel,
            attachment.mime_type,
            attachment.size_bytes.unwrap_or(0),
        );
        let content = format!("{header}{text}");

        write_document_to_workspace(workspace, &path, &content, text.len()).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_document_path, get_valid_document_text, is_usable_extracted_text, sanitise_filename,
    };
    use crate::channels::{AttachmentKind, IncomingAttachment};

    fn make_attachment(
        id: &str,
        filename: Option<&str>,
        extracted_text: Option<&str>,
    ) -> IncomingAttachment {
        IncomingAttachment {
            id: id.to_string(),
            kind: AttachmentKind::Document,
            mime_type: "application/pdf".to_string(),
            filename: filename.map(ToString::to_string),
            size_bytes: Some(42),
            source_url: None,
            storage_key: None,
            extracted_text: extracted_text.map(ToString::to_string),
            data: Vec::new(),
            duration_secs: None,
        }
    }

    #[test]
    fn usable_extracted_text_rejects_error_sentinels() {
        assert!(!is_usable_extracted_text("[Failed to extract]"));
        assert!(!is_usable_extracted_text("[Error parsing]"));
        assert!(!is_usable_extracted_text("[Unsupported format]"));
    }

    #[test]
    fn usable_extracted_text_accepts_normal_text() {
        assert!(is_usable_extracted_text("hello world"));
    }

    #[test]
    fn get_valid_document_text_filters_sentinel_outputs() {
        let attachment = make_attachment("id", Some("doc.pdf"), Some("[Failed to extract]"));
        assert_eq!(get_valid_document_text(&attachment), None);
    }

    #[test]
    fn get_valid_document_text_returns_usable_text() {
        let attachment = make_attachment("id", Some("doc.pdf"), Some("actual text"));
        assert_eq!(get_valid_document_text(&attachment), Some("actual text"));
    }

    #[test]
    fn sanitise_filename_removes_parent_traversal_segments() {
        let filename = sanitise_filename("foo/../secret");
        assert!(!filename.contains(".."));
    }

    #[test]
    fn sanitise_filename_replaces_confusable_slashes() {
        assert_eq!(sanitise_filename("foo/\u{2215}bar"), "foo__bar");
    }

    #[test]
    fn sanitise_filename_hardens_leading_traversal() {
        let filename = sanitise_filename("../etc/passwd");
        assert!(!filename.contains(".."));
        assert!(!filename.contains('/'));
        assert!(!filename.contains('\\'));
        assert!(!filename.is_empty());
    }

    #[test]
    fn sanitise_filename_preserves_normal_filenames() {
        assert_eq!(sanitise_filename("report.txt"), "report.txt");
    }

    #[test]
    fn sanitise_filename_defaults_when_empty() {
        assert_eq!(sanitise_filename(""), "unnamed_document");
    }

    #[test]
    fn build_document_path_uses_sanitized_id_and_filename() {
        let attachment = make_attachment("abc/../123", Some("../report.pdf"), Some("text"));
        let path = build_document_path(7, &attachment, "2026-04-03");
        assert!(path.starts_with("documents/2026-04-03/7-"));
        assert!(!path.contains(".."));
        let suffix = path
            .strip_prefix("documents/2026-04-03/")
            .expect("path should include date prefix");
        assert!(!suffix.contains('/'));
        assert!(!suffix.contains('\\'));
    }
}
