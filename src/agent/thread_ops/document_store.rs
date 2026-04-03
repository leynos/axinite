//! Extracted-document storage helpers for thread operations.

use std::sync::Arc;

use crate::channels::{IncomingAttachment, IncomingMessage};

/// Replace characters that are unsafe in file-system paths with an underscore.
fn sanitise_filename_char(c: char) -> char {
    if matches!(c, '/' | '\\' | '\0') {
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

    format!("documents/{date}/{index}-{}-{filename}", attachment.id)
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
    for (index, attachment) in message.attachments.iter().enumerate() {
        if attachment.kind != crate::channels::AttachmentKind::Document {
            continue;
        }
        let Some(text) = get_valid_document_text(attachment) else {
            continue;
        };

        let date = chrono::Utc::now().format("%Y-%m-%d");
        let filename =
            sanitise_filename(attachment.filename.as_deref().unwrap_or("unnamed_document"));
        let path = build_document_path(index, attachment, &date.to_string());

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
