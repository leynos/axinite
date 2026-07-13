//! Validation of registry manifests and artifact URLs prior to installation,
//! plus the policy for when artifact failures may fall back to source builds.

use std::net::IpAddr;
use std::path::{Component, Path};

use crate::registry::catalog::RegistryError;
use crate::registry::manifest::{ExtensionManifest, ManifestKind};

// GitHub-only by design. New trusted hosts (e.g. a NEAR AI CDN) must be
// explicitly added here; unknown hosts fall back to source build with a
// warning rather than surfacing a clear "host not allowed" error.
const ALLOWED_ARTIFACT_HOSTS: &[&str] = &[
    "github.com",
    "objects.githubusercontent.com",
    "github-releases.githubusercontent.com",
    "raw.githubusercontent.com",
];

pub(super) fn should_attempt_source_fallback(err: &RegistryError) -> bool {
    match err {
        // `releases/latest` is a moving target: every new release rebuilds WASM
        // extensions, so a mismatch against a `latest` URL just means the binary
        // was compiled against an older release's checksum. Not a security concern
        // — fall back to building from source.
        //
        // Version-pinned URLs (`releases/download/vX.Y.Z/`) point to an immutable
        // asset; a mismatch there is genuinely suspicious and remains a hard block.
        RegistryError::ChecksumMismatch { url, .. } => {
            url.contains("github.com/nearai/ironclaw/releases/latest/")
        }
        // Never fall back for these — they signal a structural problem or a
        // deliberate "already done" state, not a transient artifact issue.
        RegistryError::AlreadyInstalled { .. } | RegistryError::InvalidManifest { .. } => false,
        _ => true,
    }
}

fn is_allowed_artifact_host(host: &str) -> bool {
    ALLOWED_ARTIFACT_HOSTS
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
        || host.ends_with(".githubusercontent.com")
}

pub(super) fn validate_artifact_url(
    manifest_name: &str,
    field: &'static str,
    url: &str,
) -> Result<(), RegistryError> {
    let parsed = reqwest::Url::parse(url).map_err(|e| RegistryError::InvalidManifest {
        name: manifest_name.to_string(),
        field,
        reason: format!("invalid URL: {}", e),
    })?;

    if parsed.scheme() != "https" {
        return Err(RegistryError::InvalidManifest {
            name: manifest_name.to_string(),
            field,
            reason: "URL must use https".to_string(),
        });
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| RegistryError::InvalidManifest {
            name: manifest_name.to_string(),
            field,
            reason: "URL host is missing".to_string(),
        })?;

    if host.parse::<IpAddr>().is_ok() || !is_allowed_artifact_host(host) {
        return Err(RegistryError::InvalidManifest {
            name: manifest_name.to_string(),
            field,
            reason: format!("host '{}' is not allowed", host),
        });
    }

    Ok(())
}

pub(super) fn validate_manifest_install_inputs(
    manifest: &ExtensionManifest,
) -> Result<(), RegistryError> {
    let is_valid_name = !manifest.name.is_empty()
        && manifest
            .name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');

    if !is_valid_name {
        return Err(RegistryError::InvalidManifest {
            name: manifest.name.clone(),
            field: "name",
            reason: "name must contain only lowercase letters, digits, '-' or '_'".to_string(),
        });
    }

    let expected_prefix = match manifest.kind {
        ManifestKind::Tool => "tools-src/",
        ManifestKind::Channel => "channels-src/",
    };

    if !manifest.source.dir.starts_with(expected_prefix) {
        return Err(RegistryError::InvalidManifest {
            name: manifest.name.clone(),
            field: "source.dir",
            reason: format!("must start with '{}'", expected_prefix),
        });
    }

    let source_path = Path::new(&manifest.source.dir);
    let has_unsafe_component = source_path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) | Component::CurDir
        )
    });

    if source_path.is_absolute() || has_unsafe_component {
        return Err(RegistryError::InvalidManifest {
            name: manifest.name.clone(),
            field: "source.dir",
            reason: "must be a safe relative path without traversal segments".to_string(),
        });
    }

    let has_path_separator = manifest.source.capabilities.contains('/')
        || manifest.source.capabilities.contains('\\')
        || manifest.source.capabilities.contains("..");

    if has_path_separator {
        return Err(RegistryError::InvalidManifest {
            name: manifest.name.clone(),
            field: "source.capabilities",
            reason: "must be a file name without path separators".to_string(),
        });
    }

    Ok(())
}

pub(super) fn download_failure_reason(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "request timed out".to_string()
    } else if error.is_connect() {
        "connection failed".to_string()
    } else if error.is_request() {
        "request failed".to_string()
    } else {
        "network error".to_string()
    }
}
