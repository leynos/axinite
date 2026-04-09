//! Extracted-document storage helpers for thread operations.
//!
//! This module persists text extracted from document attachments (PDFs, text files, etc.)
//! into a workspace-level store for future retrieval and search. When a message contains
//! document attachments, the extracted text is written to the workspace at paths following
//! the scheme: `documents/{YYYY-MM-DD}/{index}-{message_id}-{sanitised_id}-{sanitised_filename}`.
//!
//! Path structure:
//! - Documents are organized by date in `documents/{date}/` subdirectories
//! - Each document filename includes: index (attachment position), message-level unique identifier,
//!   sanitized attachment ID, and sanitized original filename
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
pub(crate) struct PathParts<'a> {
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
pub(crate) fn build_document_path(parts: &PathParts) -> String {
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
pub(crate) fn sanitise_filename_char(c: char) -> char {
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
pub(crate) fn is_usable_extracted_text(t: &str) -> bool {
    !t.starts_with("[Failed")
        && !t.starts_with("[Error")
        && !t.starts_with("[Unsupported")
        && !t.starts_with("[Document")
}

pub(crate) fn get_valid_document_text(attachment: &IncomingAttachment) -> Option<&str> {
    match &attachment.extracted_text {
        Some(t) if is_usable_extracted_text(t) => Some(t),
        _ => None,
    }
}

pub(crate) fn sanitise_filename(raw_name: &str) -> String {
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
    let message_id = sanitise_filename(&message.id.to_string());

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
            message_id: &message_id,
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
#[path = "document_store/tests/mod.rs"]
mod tests;
