//! Temporary trace-file helpers for tests that exercise trace loading.

use std::io::Write;

use tempfile::NamedTempFile;

/// Write JSON into a named temporary file and return the open handle.
pub fn write_tmp_trace(json: &str) -> std::io::Result<NamedTempFile> {
    let mut file = NamedTempFile::new()?;
    file.write_all(json.as_bytes())?;
    Ok(file)
}
