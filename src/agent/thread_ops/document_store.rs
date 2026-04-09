//! Extracted-document storage helpers for thread operations.
//!
//! This module persists text extracted from document attachments (PDFs, text files, etc.)
//! into a workspace-level store for future retrieval and search. When a message contains
//! document attachments, the extracted text is written to the workspace at paths following
//! the scheme: `documents/{YYYY-MM-DD}/{index}-{sanitized_id}-{sanitized_filename}`.
//!
//! Path structure:
//! - Documents are organized by date in `documents/{date}/` subdirectories
//! - Each document filename includes: index (attachment position), sanitized attachment ID,
//!   and sanitized original filename
//! - All path components are sanitized via `sanitise_filename()` to remove path traversal
//!   sequences (`..`), directory separators (`/`, `\`, Unicode variants), and leading dots
//!
//! Sanitization applied:
//! - Path traversal sequences (`..`) are replaced with `__`
//! - Directory separators and lookalikes are replaced with `_`
//! - Leading dots are stripped (preventing hidden files)
//! - Empty names fall back to `unnamed_document` or `unnamed_id`
//!
//! The main entry point is `store_extracted_documents()`, which iterates message attachments,
//! filters to document types with usable extracted text (skipping error sentinels like
//! "[Failed to extract]"), builds metadata headers, and writes to the workspace.

use std::sync::Arc;

use crate::channels::{IncomingAttachment, IncomingMessage};

/// Metadata for building a document header.
struct HeaderMeta<'a> {
    filename: &'a str,
    user_id: &'a str,
    channel: &'a str,
    date: chrono::NaiveDate,
    mime: &'a str,
    size_bytes: u64,
}

/// Components for building a document path.
struct PathParts<'a> {
    date: chrono::NaiveDate,
    index: usize,
    id: &'a str,
    filename: &'a str,
    /// Message-level unique identifier to prevent collisions for unnamed attachments.
    message_id: &'a str,
}

/// Specification for writing a document to workspace.
struct DocumentWriteSpec {
    path: String,
    content: String,
    text_len: usize,
}

/// Build the document header string from metadata.
fn build_header(meta: &HeaderMeta) -> String {
    format!(
        "# {filename}\n\n\
         > Uploaded by **{user_id}** via **{channel}** on {date}\n\
         > MIME: {mime} | Size: {size_bytes} bytes\n\n---\n\n",
        filename = meta.filename,
        user_id = meta.user_id,
        channel = meta.channel,
        date = meta.date,
        mime = meta.mime,
        size_bytes = meta.size_bytes,
    )
}

/// Build the document path from parts.
fn build_document_path(parts: &PathParts) -> String {
    let date_str = parts.date.to_string();
    format!(
        "documents/{date}/{index}-{message_id}-{id}-{filename}",
        date = date_str,
        index = parts.index,
        message_id = parts.message_id,
        id = parts.id,
        filename = parts.filename
    )
}

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
    !t.starts_with("[Failed")
        && !t.starts_with("[Error")
        && !t.starts_with("[Unsupported")
        && !t.starts_with("[Document")
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

async fn write_document_to_workspace(
    workspace: &Arc<crate::workspace::Workspace>,
    spec: &DocumentWriteSpec,
) {
    match workspace.write(&spec.path, &spec.content).await {
        Ok(_) => {
            tracing::info!(
                path = %spec.path,
                text_len = spec.text_len,
                "Stored extracted document in workspace memory"
            );
        }
        Err(e) => {
            tracing::warn!(
                path = %spec.path,
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
    let today = chrono::Utc::now().date_naive();

    for (index, attachment) in message.attachments.iter().enumerate() {
        if attachment.kind != crate::channels::AttachmentKind::Document {
            continue;
        }
        let Some(text) = get_valid_document_text(attachment) else {
            continue;
        };

        let filename =
            sanitise_filename(attachment.filename.as_deref().unwrap_or("unnamed_document"));
        let sanitized_id = sanitise_filename(if attachment.id.is_empty() {
            "unnamed_id"
        } else {
            &attachment.id
        });

        let path = build_document_path(&PathParts {
            date: today,
            index,
            id: &sanitized_id,
            filename: &filename,
            message_id: &message.id.to_string(),
        });

        let header = build_header(&HeaderMeta {
            filename: &filename,
            user_id: &message.user_id,
            channel: &message.channel,
            date: today,
            mime: &attachment.mime_type,
            size_bytes: attachment.size_bytes.unwrap_or(0),
        });

        let content = format!("{header}{text}");
        let spec = DocumentWriteSpec {
            path,
            content,
            text_len: text.len(),
        };
        write_document_to_workspace(workspace, &spec).await;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        PathParts, build_document_path, get_valid_document_text, is_usable_extracted_text,
        sanitise_filename, store_extracted_documents,
    };
    use crate::channels::{AttachmentKind, IncomingAttachment};
    use crate::workspace::Workspace;

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
        assert!(!is_usable_extracted_text("[Document too large to process]"));
        assert!(!is_usable_extracted_text("[Document has no inline data]"));
    }

    #[test]
    fn usable_extracted_text_accepts_normal_text() {
        assert!(is_usable_extracted_text("hello world"));
    }

    #[test]
    fn get_valid_document_text_filters_sentinel_outputs() {
        let attachment = make_attachment("id", Some("doc.pdf"), Some("[Failed to extract]"));
        assert_eq!(get_valid_document_text(&attachment), None);

        let attachment2 = make_attachment("id2", Some("doc2.pdf"), Some("[Document too large]"));
        assert_eq!(get_valid_document_text(&attachment2), None);
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
        let date = chrono::NaiveDate::from_ymd_opt(2026, 4, 3).expect("2026-04-03 is a valid date");
        let sanitized_id = sanitise_filename("abc/../123");
        let sanitized_filename = sanitise_filename("../report.pdf");
        let path = build_document_path(&PathParts {
            date,
            index: 7,
            id: &sanitized_id,
            filename: &sanitized_filename,
            message_id: "msg-uuid-123",
        });
        assert!(path.starts_with("documents/2026-04-03/7-"));
        assert!(!path.contains(".."));
        let suffix = path
            .strip_prefix("documents/2026-04-03/")
            .expect("path should include date prefix");
        assert!(!suffix.contains('/'));
        assert!(!suffix.contains('\\'));
    }

    #[tokio::test]
    async fn store_extracted_documents_filters_and_stores_correctly() {
        use std::sync::Arc;

        use crate::channels::AttachmentKind;
        use crate::db::Database;
        use chrono::Utc;
        use uuid::Uuid;

        // Create local libSQL backend with temp file and workspace
        let tmp_dir = tempfile::tempdir().expect("create tempdir");
        let db_path = tmp_dir.path().join("doc_store_test.db");
        let backend = crate::db::libsql::LibSqlBackend::new_local(&db_path)
            .await
            .expect("failed to create local backend");
        Database::run_migrations(&backend)
            .await
            .expect("failed to run migrations");
        let workspace = Arc::new(Workspace::new_with_db("test-user", Arc::new(backend)));

        // Build message with mixed attachments
        let message_id = Uuid::new_v4();
        let message = crate::channels::IncomingMessage {
            id: message_id,
            channel: "test-channel".to_string(),
            user_id: "test-user".to_string(),
            user_name: None,
            content: "test message".to_string(),
            thread_id: None,
            received_at: Utc::now(),
            timezone: None,
            metadata: serde_json::Value::Null,
            attachments: vec![
                // Document with usable text (should be stored)
                IncomingAttachment {
                    id: "doc1".to_string(),
                    kind: AttachmentKind::Document,
                    mime_type: "application/pdf".to_string(),
                    filename: Some("report.pdf".to_string()),
                    size_bytes: Some(1024),
                    source_url: None,
                    storage_key: None,
                    extracted_text: Some("This is extracted document text".to_string()),
                    data: Vec::new(),
                    duration_secs: None,
                },
                // Non-document attachment (should be skipped)
                IncomingAttachment {
                    id: "audio1".to_string(),
                    kind: AttachmentKind::Audio,
                    mime_type: "audio/mpeg".to_string(),
                    filename: Some("recording.mp3".to_string()),
                    size_bytes: Some(2048),
                    source_url: None,
                    storage_key: None,
                    extracted_text: Some("some transcript".to_string()),
                    data: Vec::new(),
                    duration_secs: Some(60),
                },
                // Document with sentinel extracted text (should be skipped)
                IncomingAttachment {
                    id: "doc2".to_string(),
                    kind: AttachmentKind::Document,
                    mime_type: "application/pdf".to_string(),
                    filename: Some("failed.pdf".to_string()),
                    size_bytes: Some(512),
                    source_url: None,
                    storage_key: None,
                    extracted_text: Some("[Failed to extract]".to_string()),
                    data: Vec::new(),
                    duration_secs: None,
                },
                // Document without extracted text (should be skipped)
                IncomingAttachment {
                    id: "doc3".to_string(),
                    kind: AttachmentKind::Document,
                    mime_type: "application/pdf".to_string(),
                    filename: Some("no_text.pdf".to_string()),
                    size_bytes: Some(256),
                    source_url: None,
                    storage_key: None,
                    extracted_text: None,
                    data: Vec::new(),
                    duration_secs: None,
                },
            ],
        };

        store_extracted_documents(&workspace, &message).await;

        // Query workspace for stored documents
        let paths = workspace.list_all().await.expect("failed to list paths");

        // Only one document should be stored (doc1 with usable text)
        assert_eq!(paths.len(), 1, "expected exactly one stored document");
        assert!(
            paths[0].contains("doc1"),
            "stored path should contain doc1 id"
        );
        assert!(
            paths[0].contains("report.pdf"),
            "stored path should contain sanitized filename"
        );
        assert!(
            paths[0].contains(&message_id.to_string()),
            "stored path should contain message_id"
        );

        // Verify content
        let doc = workspace
            .read(&paths[0])
            .await
            .expect("failed to read document");
        assert!(
            doc.content.contains("This is extracted document text"),
            "document should contain extracted text"
        );
        assert!(
            doc.content.contains("report.pdf"),
            "document should contain filename in header"
        );
        assert!(
            doc.content.contains("test-user"),
            "document should contain user_id in header"
        );
    }
}
