//! Tests for extracted-document storage helpers.

use rstest::rstest;

use crate::channels::{AttachmentKind, IncomingAttachment};
use crate::workspace::Workspace;

use super::{
    PathParts, build_document_path, get_valid_document_text, is_usable_extracted_text,
    sanitise_filename, store_extracted_documents,
};

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

#[rstest]
#[case("[Failed to extract]")]
#[case("[Error parsing]")]
#[case("[Unsupported format]")]
#[case("[Document too large to process]")]
#[case("[Document has no inline data]")]
fn is_usable_extracted_text_rejects_sentinels(#[case] sentinel: &str) {
    assert!(
        !is_usable_extracted_text(sentinel),
        "should reject sentinel: {sentinel}"
    );
    let attachment = make_attachment("id", Some("doc.pdf"), Some(sentinel));
    assert_eq!(
        get_valid_document_text(&attachment),
        None,
        "get_valid_document_text should return None for sentinel: {sentinel}"
    );
}

#[rstest]
#[case("hello world")]
#[case("actual text")]
#[case("Some normal document content")]
fn is_usable_extracted_text_accepts_valid_text(#[case] text: &str) {
    assert!(
        is_usable_extracted_text(text),
        "should accept valid text: {text}"
    );
    let attachment = make_attachment("id", Some("doc.pdf"), Some(text));
    assert_eq!(
        get_valid_document_text(&attachment),
        Some(text),
        "get_valid_document_text should return Some for valid text"
    );
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
