//! Pre-built artifact installation path: download, checksum verification,
//! and fallback to build-from-source when artifacts are unavailable.

use tokio::fs;

use crate::registry::catalog::RegistryError;
use crate::registry::manifest::{ArtifactSpec, ExtensionManifest};

use super::archive::extract_tar_gz;
use super::validation::{
    download_failure_reason, should_attempt_source_fallback, validate_artifact_url,
    validate_manifest_install_inputs,
};
use super::{InstallOutcome, RegistryInstaller};

impl RegistryInstaller {
    pub async fn install_with_source_fallback(
        &self,
        manifest: &ExtensionManifest,
        force: bool,
    ) -> Result<InstallOutcome, RegistryError> {
        // Validate upfront so we fail fast on bad manifests regardless of
        // which install path runs, without relying on inner methods to
        // catch it first.
        validate_manifest_install_inputs(manifest)?;

        let has_artifact = manifest
            .artifacts
            .get("wasm32-wasip2")
            .and_then(|a| a.url.as_ref())
            .is_some();

        if !has_artifact {
            return self.install_from_source(manifest, force).await;
        }

        let source_dir = self.repo_root.join(&manifest.source.dir);

        match self.install_from_artifact(manifest, force).await {
            Ok(outcome) => Ok(outcome),
            Err(artifact_err) => {
                if !should_attempt_source_fallback(&artifact_err) {
                    return Err(artifact_err);
                }

                if !source_dir.is_dir() {
                    return Err(RegistryError::SourceFallbackUnavailable {
                        name: manifest.name.clone(),
                        source_dir,
                        artifact_error: Box::new(artifact_err),
                    });
                }

                tracing::warn!(
                    extension = %manifest.name,
                    error = %artifact_err,
                    "Artifact install failed; falling back to build-from-source"
                );

                match self.install_from_source(manifest, force).await {
                    Ok(mut outcome) => {
                        outcome.warnings.push(format!(
                            "Artifact install failed ({}); installed via source fallback.",
                            artifact_err
                        ));
                        Ok(outcome)
                    }
                    Err(source_err) => Err(RegistryError::InstallFallbackFailed {
                        name: manifest.name.clone(),
                        artifact_error: Box::new(artifact_err),
                        source_error: Box::new(source_err),
                    }),
                }
            }
        }
    }

    /// Download and install a pre-built artifact.
    ///
    /// Supports two formats:
    /// - **tar.gz bundle**: Contains `{name}.wasm` + `{name}.capabilities.json`
    /// - **bare .wasm file**: Just the WASM binary (capabilities fetched separately if available)
    pub async fn install_from_artifact(
        &self,
        manifest: &ExtensionManifest,
        force: bool,
    ) -> Result<InstallOutcome, RegistryError> {
        validate_manifest_install_inputs(manifest)?;

        let (artifact, url, expected_sha) = resolve_verified_artifact(manifest)?;

        let target_wasm = self.prepare_install_target(manifest, force).await?;
        let target_caps =
            target_wasm.with_file_name(format!("{}.capabilities.json", manifest.name));

        // Download
        println!(
            "Downloading {} '{}'...",
            manifest.kind, manifest.display_name
        );
        let bytes = download_artifact(url).await?;
        verify_sha256(&bytes, expected_sha, url)?;

        // Detect format and extract
        let has_capabilities = if is_gzip(&bytes) {
            // tar.gz bundle: extract {name}.wasm and {name}.capabilities.json
            let extracted =
                extract_tar_gz(&bytes, &manifest.name, &target_wasm, &target_caps, url)?;
            extracted.has_capabilities
        } else {
            self.install_bare_wasm(manifest, artifact, &bytes, &target_wasm, &target_caps)
                .await?
        };

        println!("  Installed to {}", target_wasm.display());

        let mut warnings = Vec::new();
        if !has_capabilities {
            warnings.push(format!(
                "No capabilities file found for '{}'. Auth and hooks may not work.",
                manifest.name
            ));
        }

        Ok(InstallOutcome {
            name: manifest.name.clone(),
            kind: manifest.kind,
            wasm_path: target_wasm,
            has_capabilities,
            warnings,
        })
    }

    /// Install a bare (non-archive) WASM artifact, then source capabilities
    /// from the artifact's `capabilities_url` or, failing that, the legacy
    /// source tree. Returns whether capabilities were installed.
    async fn install_bare_wasm(
        &self,
        manifest: &ExtensionManifest,
        artifact: &ArtifactSpec,
        bytes: &[u8],
        target_wasm: &std::path::Path,
        target_caps: &std::path::Path,
    ) -> Result<bool, RegistryError> {
        fs::write(target_wasm, bytes)
            .await
            .map_err(RegistryError::Io)?;

        // Try to get capabilities from:
        // 1. Separate capabilities_url in the artifact
        // 2. Source tree (legacy, requires repo)
        if let Some(ref caps_url) = artifact.capabilities_url {
            validate_artifact_url(
                &manifest.name,
                "artifacts.wasm32-wasip2.capabilities_url",
                caps_url,
            )?;
            download_capabilities(caps_url, target_caps).await
        } else {
            self.copy_legacy_capabilities(manifest, target_caps).await
        }
    }

    /// Legacy fallback: copy the capabilities file from the source tree, if
    /// present. Returns whether a file was copied.
    async fn copy_legacy_capabilities(
        &self,
        manifest: &ExtensionManifest,
        target_caps: &std::path::Path,
    ) -> Result<bool, RegistryError> {
        let caps_source = self
            .repo_root
            .join(&manifest.source.dir)
            .join(&manifest.source.capabilities);
        if !caps_source.exists() {
            return Ok(false);
        }
        fs::copy(&caps_source, target_caps)
            .await
            .map_err(RegistryError::Io)?;
        Ok(true)
    }
}

/// Resolve the wasm32-wasip2 artifact for a manifest, validating its URL and
/// requiring a SHA256 checksum. Returns the artifact together with its URL
/// and expected checksum.
///
/// The checksum is required — installing unverified binaries is refused, and
/// the check happens before downloading to avoid wasting bandwidth. It uses
/// `MissingChecksum` (not `InvalidManifest`) so that
/// `install_with_source_fallback` can fall back to building from source when
/// checksums haven't been populated yet (bootstrapping).
fn resolve_verified_artifact(
    manifest: &ExtensionManifest,
) -> Result<(&ArtifactSpec, &str, &str), RegistryError> {
    let artifact = manifest.artifacts.get("wasm32-wasip2").ok_or_else(|| {
        RegistryError::ExtensionNotFound(format!(
            "No wasm32-wasip2 artifact for '{}'",
            manifest.name
        ))
    })?;

    let url = artifact.url.as_ref().ok_or_else(|| {
        RegistryError::ExtensionNotFound(format!(
            "No artifact URL for '{}'. Use --build to build from source.",
            manifest.name
        ))
    })?;

    validate_artifact_url(&manifest.name, "artifacts.wasm32-wasip2.url", url)?;

    let expected_sha = artifact
        .sha256
        .as_ref()
        .ok_or_else(|| RegistryError::MissingChecksum {
            name: manifest.name.clone(),
        })?;

    Ok((artifact, url, expected_sha))
}

/// Download a capabilities file (capped at 1 MB) to `target_caps`.
///
/// Download failures and oversized files are logged and reported as "no
/// capabilities" (best-effort for bare artifacts); local write errors are
/// propagated.
async fn download_capabilities(
    caps_url: &str,
    target_caps: &std::path::Path,
) -> Result<bool, RegistryError> {
    const MAX_CAPS_SIZE: usize = 1024 * 1024; // 1 MB
    match download_artifact(caps_url).await {
        Ok(caps_bytes) if caps_bytes.len() <= MAX_CAPS_SIZE => {
            fs::write(target_caps, &caps_bytes)
                .await
                .map_err(RegistryError::Io)?;
            Ok(true)
        }
        Ok(caps_bytes) => {
            tracing::warn!(
                "Capabilities file too large ({} bytes, max {}), skipping",
                caps_bytes.len(),
                MAX_CAPS_SIZE
            );
            Ok(false)
        }
        Err(e) => {
            tracing::warn!("Failed to download capabilities from {}: {}", caps_url, e);
            Ok(false)
        }
    }
}

/// Download an artifact from a URL.
async fn download_artifact(url: &str) -> Result<bytes::Bytes, RegistryError> {
    let response = reqwest::get(url)
        .await
        .map_err(|e| RegistryError::DownloadFailed {
            url: url.to_string(),
            reason: download_failure_reason(&e),
        })?;

    let response = response
        .error_for_status()
        .map_err(|e| RegistryError::DownloadFailed {
            url: url.to_string(),
            reason: format!(
                "http status {}",
                e.status()
                    .map_or("unknown".to_string(), |status| status.as_u16().to_string())
            ),
        })?;

    response
        .bytes()
        .await
        .map_err(|e| RegistryError::DownloadFailed {
            url: url.to_string(),
            reason: format!("failed to read response body: {}", e),
        })
}

/// Verify SHA256 of downloaded bytes.
pub(super) fn verify_sha256(bytes: &[u8], expected: &str, url: &str) -> Result<(), RegistryError> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let actual = format!("{:x}", hasher.finalize());

    if actual != expected {
        return Err(RegistryError::ChecksumMismatch {
            url: url.to_string(),
            expected_sha256: expected.to_string(),
            actual_sha256: actual,
        });
    }
    Ok(())
}

/// Check if bytes start with gzip magic number (0x1f 0x8b).
pub(super) fn is_gzip(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b
}
