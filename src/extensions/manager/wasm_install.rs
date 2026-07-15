//! WASM artifact download, gzip extraction, and build-from-source installation.

use crate::extensions::{ExtensionError, ExtensionKind, InstallResult};

use super::ExtensionManager;
use super::sanitize_url_for_logging;

/// 100 MB cap on a single decompressed tar entry to prevent decompression bombs.
const MAX_TAR_ENTRY_SIZE: u64 = 100 * 1024 * 1024;

/// Read a single tar entry (bounded by [`MAX_TAR_ENTRY_SIZE`]) and write it to
/// `dest`, without preserving permissions or extended attributes.
fn extract_tar_entry<R: std::io::Read>(
    entry: &mut tar::Entry<'_, R>,
    dest: &std::path::Path,
) -> Result<(), ExtensionError> {
    use std::io::Read as _;

    let mut data = Vec::with_capacity(entry.size() as usize);
    entry
        .by_ref()
        .take(MAX_TAR_ENTRY_SIZE)
        .read_to_end(&mut data)
        .map_err(|e| ExtensionError::InstallFailed(e.to_string()))?;
    ambient_fs::write(dest, &data).map_err(|e| ExtensionError::InstallFailed(e.to_string()))
}

impl ExtensionManager {
    /// Whether the payload begins with the gzip magic number (`0x1f 0x8b`).
    pub(super) fn is_gzip_payload(bytes: &[u8]) -> bool {
        bytes.starts_with(&[0x1f, 0x8b])
    }

    /// Download a WASM extension (tool or channel) from URL and install to target directory.
    ///
    /// Handles both tar.gz bundles (containing `.wasm` + `.capabilities.json`) and bare
    /// `.wasm` files. Validates HTTPS, size limits, and file format.
    pub(super) async fn download_and_install_wasm(
        &self,
        name: &str,
        url: &str,
        capabilities_url: Option<&str>,
        target_dir: &std::path::Path,
    ) -> Result<(), ExtensionError> {
        // Require HTTPS to prevent downgrade attacks
        if !url.starts_with("https://") {
            return Err(ExtensionError::InstallFailed(
                "Only HTTPS URLs are allowed for extension downloads".to_string(),
            ));
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| ExtensionError::DownloadFailed(e.to_string()))?;

        let bytes = Self::fetch_extension_bytes(&client, name, url).await?;

        // Ensure target directory exists
        tokio::fs::create_dir_all(target_dir)
            .await
            .map_err(|e| ExtensionError::InstallFailed(e.to_string()))?;

        let wasm_path = target_dir.join(format!("{}.wasm", name));
        let caps_path = target_dir.join(format!("{}.capabilities.json", name));

        // Detect format: gzip (tar.gz bundle) or bare WASM
        if Self::is_gzip_payload(&bytes) {
            // tar.gz bundle: extract {name}.wasm and {name}.capabilities.json
            self.extract_wasm_tar_gz(name, &bytes, &wasm_path, &caps_path)?;
        } else {
            Self::write_bare_wasm(&bytes, &wasm_path).await?;

            // Download capabilities separately if URL provided
            if let Some(caps_url) = capabilities_url {
                Self::download_capabilities(&client, name, caps_url, &caps_path).await;
            }
        }

        tracing::info!(
            "Installed WASM extension '{}' from {} to {}",
            name,
            url,
            wasm_path.display()
        );

        Ok(())
    }

    /// Fetch the extension payload, enforcing HTTP success and a 50 MB size
    /// cap (checked against Content-Length and the actual body).
    async fn fetch_extension_bytes(
        client: &reqwest::Client,
        name: &str,
        url: &str,
    ) -> Result<bytes::Bytes, ExtensionError> {
        // 50 MB cap to prevent disk-fill DoS
        const MAX_DOWNLOAD_SIZE: usize = 50 * 1024 * 1024;

        let sanitized_url = sanitize_url_for_logging(url);
        tracing::debug!(extension = %name, url = %sanitized_url, "Downloading WASM extension");

        let response = client.get(url).send().await.map_err(|e| {
            tracing::error!(extension = %name, url = %sanitized_url, error = %e, "Download request failed");
            ExtensionError::DownloadFailed(e.to_string())
        })?;

        if !response.status().is_success() {
            let status = response.status();
            tracing::error!(
                extension = %name,
                url = %sanitized_url,
                status = %status,
                "Download returned non-success HTTP status"
            );
            return Err(ExtensionError::DownloadFailed(format!(
                "HTTP {} from {}",
                status, url
            )));
        }

        // Check Content-Length header before downloading the full body
        if let Some(len) = response.content_length()
            && len as usize > MAX_DOWNLOAD_SIZE
        {
            return Err(ExtensionError::InstallFailed(format!(
                "Download too large ({} bytes, max {} bytes)",
                len, MAX_DOWNLOAD_SIZE
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ExtensionError::DownloadFailed(e.to_string()))?;

        if bytes.len() > MAX_DOWNLOAD_SIZE {
            return Err(ExtensionError::InstallFailed(format!(
                "Download too large ({} bytes, max {} bytes)",
                bytes.len(),
                MAX_DOWNLOAD_SIZE
            )));
        }
        Ok(bytes)
    }

    /// Validate the WASM magic number and write a bare `.wasm` payload.
    async fn write_bare_wasm(
        bytes: &[u8],
        wasm_path: &std::path::Path,
    ) -> Result<(), ExtensionError> {
        if bytes.len() < 4 || &bytes[..4] != b"\0asm" {
            return Err(ExtensionError::InstallFailed(
                "Downloaded file is not a valid WASM binary (bad magic number)".to_string(),
            ));
        }

        tokio::fs::write(wasm_path, bytes)
            .await
            .map_err(|e| ExtensionError::InstallFailed(e.to_string()))
    }

    /// Best-effort download of the separate capabilities file (warns on any
    /// failure rather than failing the install).
    async fn download_capabilities(
        client: &reqwest::Client,
        name: &str,
        caps_url: &str,
        caps_path: &std::path::Path,
    ) {
        const MAX_CAPS_SIZE: usize = 1024 * 1024; // 1 MB

        // A failed request or non-success status share the same "from URL" warning.
        let Ok(resp) = client.get(caps_url).send().await else {
            tracing::warn!(
                "Failed to download capabilities for '{}' from {}",
                name,
                caps_url
            );
            return;
        };
        if !resp.status().is_success() {
            tracing::warn!(
                "Failed to download capabilities for '{}' from {}",
                name,
                caps_url
            );
            return;
        }

        let caps_bytes = match resp.bytes().await {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::warn!("Failed to download capabilities for '{}': {}", name, e);
                return;
            }
        };

        if caps_bytes.len() > MAX_CAPS_SIZE {
            tracing::warn!(
                "Capabilities file for '{}' too large ({} bytes, max {})",
                name,
                caps_bytes.len(),
                MAX_CAPS_SIZE
            );
            return;
        }

        if let Err(e) = tokio::fs::write(caps_path, &caps_bytes).await {
            tracing::warn!("Failed to write capabilities for '{}': {}", name, e);
        }
    }

    /// Extract a tar.gz bundle into the WASM tools directory.
    pub(super) fn extract_wasm_tar_gz(
        &self,
        name: &str,
        bytes: &[u8],
        target_wasm: &std::path::Path,
        target_caps: &std::path::Path,
    ) -> Result<(), ExtensionError> {
        use flate2::read::GzDecoder;
        use tar::Archive;

        let decoder = GzDecoder::new(bytes);
        let mut archive = Archive::new(decoder);
        // Defense-in-depth: do not preserve permissions or extended attributes
        archive.set_preserve_permissions(false);
        #[cfg(any(unix, target_os = "redox"))]
        archive.set_unpack_xattrs(false);

        let wasm_filename = format!("{}.wasm", name);
        let caps_filename = format!("{}.capabilities.json", name);
        let mut found_wasm = false;

        let entries = archive
            .entries()
            .map_err(|e| ExtensionError::InstallFailed(format!("Bad tar.gz archive: {}", e)))?;

        for entry in entries {
            let mut entry = entry
                .map_err(|e| ExtensionError::InstallFailed(format!("Bad tar.gz entry: {}", e)))?;

            if entry.size() > MAX_TAR_ENTRY_SIZE {
                return Err(ExtensionError::InstallFailed(format!(
                    "Archive entry too large ({} bytes, max {} bytes)",
                    entry.size(),
                    MAX_TAR_ENTRY_SIZE
                )));
            }

            let entry_path = entry
                .path()
                .map_err(|e| {
                    ExtensionError::InstallFailed(format!("Invalid path in tar.gz: {}", e))
                })?
                .to_path_buf();

            let filename = entry_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            if filename == wasm_filename {
                extract_tar_entry(&mut entry, target_wasm)?;
                found_wasm = true;
            } else if filename == caps_filename {
                extract_tar_entry(&mut entry, target_caps)?;
            }
        }

        if !found_wasm {
            return Err(ExtensionError::InstallFailed(format!(
                "tar.gz archive does not contain '{}'",
                wasm_filename
            )));
        }

        Ok(())
    }

    /// Install a WASM extension from local build artifacts (WasmBuildable source).
    ///
    /// Resolves the build directory (relative to `CARGO_MANIFEST_DIR` or absolute),
    /// looks for the compiled WASM artifact, and copies it (plus capabilities.json)
    /// to the install directory. Falls back to an error if artifacts don't exist.
    pub(super) async fn install_wasm_from_buildable(
        &self,
        name: &str,
        build_dir: Option<&str>,
        crate_name: Option<&str>,
        target_dir: &std::path::Path,
        kind: ExtensionKind,
    ) -> Result<InstallResult, ExtensionError> {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));

        // Resolve build directory
        let resolved_dir = match build_dir {
            Some(dir) => {
                let p = std::path::Path::new(dir);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    manifest_dir.join(dir)
                }
            }
            None => manifest_dir.to_path_buf(),
        };

        // Determine the binary name to look for
        let binary_name = crate_name.unwrap_or(name);

        let wasm_src =
            crate::registry::artifacts::find_wasm_artifact(&resolved_dir, binary_name, "release")
                .ok_or_else(|| {
                ExtensionError::InstallFailed(format!(
                    "'{}' requires building from source. Build artifact not found. \
                         Run `cargo component build --release` in {} first, \
                         or use `ironclaw registry install {}`.",
                    name,
                    resolved_dir.display(),
                    name,
                ))
            })?;

        let wasm_dst = crate::registry::artifacts::install_wasm_files(
            &wasm_src,
            &resolved_dir,
            name,
            target_dir,
            true,
        )
        .await
        .map_err(|e| ExtensionError::InstallFailed(e.to_string()))?;

        let kind_label = match kind {
            ExtensionKind::WasmTool => "WASM tool",
            ExtensionKind::WasmChannel => "WASM channel",
            ExtensionKind::McpServer => "MCP server",
            ExtensionKind::ChannelRelay => "channel relay",
        };

        tracing::info!(
            "Installed {} '{}' from build artifacts at {}",
            kind_label,
            name,
            wasm_dst.display(),
        );

        Ok(InstallResult {
            name: name.to_string(),
            kind,
            message: format!(
                "{} '{}' installed from local build artifacts. Run activate to load it.",
                kind_label, name,
            ),
        })
    }
}
