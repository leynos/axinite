//! Safe extraction of downloaded tar.gz artifact bundles.

use std::io::Read as _;
use std::path::Path;

use flate2::read::GzDecoder;
use tar::{Archive, Entry};

use crate::registry::catalog::RegistryError;

/// 100 MB cap on decompressed entry size to prevent decompression bombs.
const MAX_ENTRY_SIZE: u64 = 100 * 1024 * 1024;

/// Result of extracting a tar.gz bundle.
pub(super) struct ExtractResult {
    pub(super) has_capabilities: bool,
}

/// Build a `DownloadFailed` error for the given URL and reason.
fn download_err(url: &str, reason: String) -> RegistryError {
    RegistryError::DownloadFailed {
        url: url.to_string(),
        reason,
    }
}

/// Open a tar.gz archive over the downloaded bytes with hardened defaults.
fn open_archive(bytes: &[u8]) -> Archive<GzDecoder<&[u8]>> {
    let decoder = GzDecoder::new(bytes);
    let mut archive = Archive::new(decoder);
    // Defence-in-depth: do not preserve permissions or extended attributes
    archive.set_preserve_permissions(false);
    #[cfg(any(unix, target_os = "redox"))]
    archive.set_unpack_xattrs(false);
    archive
}

/// Return the entry's bare file name (ignoring any directory prefix), or an
/// empty string when the name is missing or not valid UTF-8.
fn entry_file_name<R: std::io::Read>(
    entry: &Entry<'_, R>,
    url: &str,
) -> Result<String, RegistryError> {
    let entry_path = entry
        .path()
        .map_err(|e| download_err(url, format!("invalid path in tar.gz: {}", e)))?;
    Ok(entry_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string())
}

/// Read a single archive entry (capped at [`MAX_ENTRY_SIZE`]) and write it
/// to `target`. Callers must have rejected oversized entries beforehand.
fn write_entry_to<R: std::io::Read>(
    entry: &mut Entry<'_, R>,
    target: &Path,
    filename: &str,
    url: &str,
) -> Result<(), RegistryError> {
    let mut data = Vec::with_capacity(entry.size() as usize);
    std::io::Read::read_to_end(&mut entry.by_ref().take(MAX_ENTRY_SIZE), &mut data).map_err(
        |e| {
            download_err(
                url,
                format!("failed to read {} from archive: {}", filename, e),
            )
        },
    )?;
    ambient_fs::write(target, &data).map_err(RegistryError::Io)?;
    Ok(())
}

/// Extract a tar.gz archive, looking for `{name}.wasm` and `{name}.capabilities.json`.
pub(super) fn extract_tar_gz(
    bytes: &[u8],
    name: &str,
    target_wasm: &Path,
    target_caps: &Path,
    url: &str,
) -> Result<ExtractResult, RegistryError> {
    let mut archive = open_archive(bytes);

    let wasm_filename = format!("{}.wasm", name);
    let caps_filename = format!("{}.capabilities.json", name);
    let mut found_wasm = false;
    let mut found_caps = false;

    let entries = archive
        .entries()
        .map_err(|e| download_err(url, format!("failed to read tar.gz entries: {}", e)))?;

    for entry in entries {
        let mut entry =
            entry.map_err(|e| download_err(url, format!("failed to read tar.gz entry: {}", e)))?;

        // Bound every entry up front so oversized entries fail even when
        // they are not one of the files we extract.
        if entry.size() > MAX_ENTRY_SIZE {
            return Err(download_err(
                url,
                format!(
                    "archive entry too large ({} bytes, max {} bytes)",
                    entry.size(),
                    MAX_ENTRY_SIZE
                ),
            ));
        }

        // Match by filename (ignoring any directory prefix in the archive)
        let filename = entry_file_name(&entry, url)?;

        if filename == wasm_filename {
            write_entry_to(&mut entry, target_wasm, &wasm_filename, url)?;
            found_wasm = true;
        } else if filename == caps_filename {
            write_entry_to(&mut entry, target_caps, &caps_filename, url)?;
            found_caps = true;
        }
    }

    if !found_wasm {
        return Err(download_err(
            url,
            format!(
                "tar.gz archive does not contain '{}'. Archive may be malformed.",
                wasm_filename
            ),
        ));
    }

    Ok(ExtractResult {
        has_capabilities: found_caps,
    })
}
