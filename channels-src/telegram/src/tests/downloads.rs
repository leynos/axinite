use crate::downloads::{
    download_and_store_documents, is_downloadable_document, MAX_DOWNLOAD_SIZE_BYTES,
};
use crate::near::agent::channel_host::InboundAttachment;

#[test]
fn test_is_downloadable_document() {
    let make = |mime: &str, filename: Option<&str>| InboundAttachment {
        id: "test".to_string(),
        mime_type: mime.to_string(),
        filename: filename.map(|s| s.to_string()),
        size_bytes: Some(1024),
        source_url: None,
        storage_key: None,
        extracted_text: None,
        extras_json: String::new(),
    };

    // PDFs and Office docs should be downloaded
    assert!(is_downloadable_document(&make(
        "application/pdf",
        Some("report.pdf")
    )));
    assert!(is_downloadable_document(&make(
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("doc.docx"),
    )));
    assert!(is_downloadable_document(&make(
        "text/plain",
        Some("notes.txt")
    )));

    // Voice, image, audio, video should NOT be downloaded
    assert!(!is_downloadable_document(&make(
        "audio/ogg",
        Some("voice_123.ogg")
    )));
    assert!(!is_downloadable_document(&make("image/jpeg", None)));
    assert!(!is_downloadable_document(&make(
        "audio/mpeg",
        Some("song.mp3")
    )));
    assert!(!is_downloadable_document(&make(
        "video/mp4",
        Some("clip.mp4")
    )));
}

#[test]
fn test_max_download_size_constant() {
    // Verify the constant is 20 MB, matching the Slack channel limit
    assert_eq!(MAX_DOWNLOAD_SIZE_BYTES, 20 * 1024 * 1024);
}

#[test]
fn test_download_and_store_documents_function_exists() {
    let attachments = &mut [InboundAttachment {
        id: "1".to_string(),
        mime_type: "application/pdf".to_string(),
        filename: Some("x".to_string()),
        size_bytes: Some(1),
        source_url: None,
        storage_key: None,
        extracted_text: None,
        extras_json: String::new(),
    }];
    download_and_store_documents(attachments);
}
