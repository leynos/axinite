use super::wit_channel;

// ============================================================================
// Attachment Helpers
// ============================================================================

/// Maximum total attachment size (50 MB).
pub(super) const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 50 * 1024 * 1024;

/// Detect MIME type from file extension using the `mime_guess` crate.
pub(super) fn mime_from_extension(path: &str) -> String {
    mime_guess::from_path(path)
        .first_or_octet_stream()
        .to_string()
}

/// Read attachment files from disk and build WIT attachment records.
///
/// Validates total size against `MAX_TOTAL_ATTACHMENT_BYTES`.
pub(super) fn read_attachments(paths: &[String]) -> Result<Vec<wit_channel::Attachment>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let mut attachments = Vec::with_capacity(paths.len());
    let mut total_bytes: u64 = 0;
    let tmp_base = std::path::Path::new("/tmp");
    let home_base = dirs::home_dir()
        .map(|h| h.join(".ironclaw"))
        .unwrap_or_default();

    for path in paths {
        // Validate paths are under /tmp/ or ~/.ironclaw/ to prevent arbitrary file reads
        let validated = crate::tools::builtin::path_utils::validate_path(path, Some(tmp_base))
            .or_else(|_| crate::tools::builtin::path_utils::validate_path(path, Some(&home_base)));
        let validated = validated.map_err(|e| {
            format!(
                "Invalid attachment path '{}': must be under /tmp/ or ~/.ironclaw/: {}",
                path, e
            )
        })?;

        // Pre-check file size before reading into memory to avoid OOM
        let file_size = std::fs::metadata(&validated)
            .map_err(|e| format!("Failed to stat attachment '{}': {}", validated.display(), e))?
            .len();
        total_bytes += file_size;
        if total_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
            return Err(format!(
                "Total attachment size exceeds {} MB limit",
                MAX_TOTAL_ATTACHMENT_BYTES / (1024 * 1024)
            ));
        }

        let data = std::fs::read(&validated)
            .map_err(|e| format!("Failed to read attachment '{}': {}", validated.display(), e))?;

        let filename = validated
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();

        let mime_type = mime_from_extension(path);

        attachments.push(wit_channel::Attachment {
            filename,
            mime_type,
            data,
        });
    }

    Ok(attachments)
}
