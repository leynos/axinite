//! Safe extraction of downloaded tar.gz artifact bundles.

use std::path::Path;

use crate::registry::catalog::RegistryError;

/// Result of extracting a tar.gz bundle.
pub(super) struct ExtractResult {
    pub(super) has_capabilities: bool,
}

/// Extract a tar.gz archive, looking for `{name}.wasm` and `{name}.capabilities.json`.
pub(super) fn extract_tar_gz(
    bytes: &[u8],
    name: &str,
    target_wasm: &Path,
    target_caps: &Path,
    url: &str,
) -> Result<ExtractResult, RegistryError> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    use std::io::Read as _;

    let decoder = GzDecoder::new(bytes);
    let mut archive = Archive::new(decoder);
    // Defense-in-depth: do not preserve permissions or extended attributes
    archive.set_preserve_permissions(false);
    #[cfg(any(unix, target_os = "redox"))]
    archive.set_unpack_xattrs(false);

    // 100 MB cap on decompressed entry size to prevent decompression bombs
    const MAX_ENTRY_SIZE: u64 = 100 * 1024 * 1024;

    let wasm_filename = format!("{}.wasm", name);
    let caps_filename = format!("{}.capabilities.json", name);
    let mut found_wasm = false;
    let mut found_caps = false;

    let entries = archive
        .entries()
        .map_err(|e| RegistryError::DownloadFailed {
            url: url.to_string(),
            reason: format!("failed to read tar.gz entries: {}", e),
        })?;

    for entry in entries {
        let mut entry = entry.map_err(|e| RegistryError::DownloadFailed {
            url: url.to_string(),
            reason: format!("failed to read tar.gz entry: {}", e),
        })?;

        if entry.size() > MAX_ENTRY_SIZE {
            return Err(RegistryError::DownloadFailed {
                url: url.to_string(),
                reason: format!(
                    "archive entry too large ({} bytes, max {} bytes)",
                    entry.size(),
                    MAX_ENTRY_SIZE
                ),
            });
        }

        let entry_path = entry
            .path()
            .map_err(|e| RegistryError::DownloadFailed {
                url: url.to_string(),
                reason: format!("invalid path in tar.gz: {}", e),
            })?
            .to_path_buf();

        // Match by filename (ignoring any directory prefix in the archive)
        let filename = entry_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        if filename == wasm_filename {
            let mut data = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry.by_ref().take(MAX_ENTRY_SIZE), &mut data)
                .map_err(|e| RegistryError::DownloadFailed {
                    url: url.to_string(),
                    reason: format!("failed to read {} from archive: {}", wasm_filename, e),
                })?;
            ambient_fs::write(target_wasm, &data).map_err(RegistryError::Io)?;
            found_wasm = true;
        } else if filename == caps_filename {
            let mut data = Vec::with_capacity(entry.size() as usize);
            std::io::Read::read_to_end(&mut entry.by_ref().take(MAX_ENTRY_SIZE), &mut data)
                .map_err(|e| RegistryError::DownloadFailed {
                    url: url.to_string(),
                    reason: format!("failed to read {} from archive: {}", caps_filename, e),
                })?;
            ambient_fs::write(target_caps, &data).map_err(RegistryError::Io)?;
            found_caps = true;
        }
    }

    if !found_wasm {
        return Err(RegistryError::DownloadFailed {
            url: url.to_string(),
            reason: format!(
                "tar.gz archive does not contain '{}'. Archive may be malformed.",
                wasm_filename
            ),
        });
    }

    Ok(ExtractResult {
        has_capabilities: found_caps,
    })
}
