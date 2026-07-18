//! Unit tests for registry manifest installation.

use super::archive::{TarGzExtraction, extract_tar_gz};
use super::artifact::{is_gzip, verify_sha256};
use super::validation::should_attempt_source_fallback;
use super::*;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::registry::catalog::RegistryError;
use crate::registry::manifest::{ArtifactSpec, ExtensionManifest, ManifestKind, SourceSpec};

/// Build the optional wasm32-wasip2 artifact spec for a test manifest.
/// Returns `None` when neither a URL nor a checksum is supplied.
fn test_artifact(url: Option<&str>, sha256: Option<&str>) -> Option<ArtifactSpec> {
    if url.is_none() && sha256.is_none() {
        return None;
    }
    Some(ArtifactSpec {
        url: url.map(ToString::to_string),
        sha256: sha256.map(ToString::to_string),
        capabilities_url: None,
    })
}

fn test_manifest(
    name: &str,
    source_dir: &str,
    artifact: Option<ArtifactSpec>,
) -> ExtensionManifest {
    test_manifest_with_kind(name, source_dir, artifact, ManifestKind::Tool)
}

fn test_manifest_with_kind(
    name: &str,
    source_dir: &str,
    artifact: Option<ArtifactSpec>,
    kind: ManifestKind,
) -> ExtensionManifest {
    let mut artifacts = HashMap::new();
    if let Some(spec) = artifact {
        artifacts.insert("wasm32-wasip2".to_string(), spec);
    }

    ExtensionManifest {
        name: name.to_string(),
        display_name: name.to_string(),
        kind,
        version: "0.1.0".to_string(),
        description: "test manifest".to_string(),
        keywords: Vec::new(),
        source: SourceSpec {
            dir: source_dir.to_string(),
            capabilities: format!("{}.capabilities.json", name),
            crate_name: name.to_string(),
        },
        artifacts,
        auth_summary: None,
        tags: Vec::new(),
    }
}

#[test]
fn test_installer_creation() {
    let installer = RegistryInstaller::new(
        PathBuf::from("/repo"),
        PathBuf::from("/home/.ironclaw/tools"),
        PathBuf::from("/home/.ironclaw/channels"),
    );
    assert_eq!(installer.repo_root, PathBuf::from("/repo"));
}

#[test]
fn test_is_gzip() {
    assert!(is_gzip(&[0x1f, 0x8b, 0x08]));
    assert!(!is_gzip(&[0x00, 0x61, 0x73, 0x6d])); // WASM magic
    assert!(!is_gzip(&[0x1f])); // Too short
    assert!(!is_gzip(&[]));
}

#[test]
fn test_verify_sha256_valid() {
    use sha2::{Digest, Sha256};
    let data = b"hello world";
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hex::encode(hasher.finalize());
    assert!(verify_sha256(data, &hash, "test://url").is_ok());
}

#[test]
fn test_verify_sha256_invalid() {
    let err = verify_sha256(b"data", "0000", "test://url").expect_err("checksum mismatch");
    assert!(matches!(err, RegistryError::ChecksumMismatch { .. }));
}

#[tokio::test]
async fn test_install_from_source_rejects_path_traversal_name() {
    let temp = tempfile::tempdir().expect("tempdir");
    let installer = RegistryInstaller::new(
        temp.path().to_path_buf(),
        temp.path().join("tools"),
        temp.path().join("channels"),
    );

    let manifest = test_manifest("../evil", "tools-src/evil", None);

    let result = installer.install_from_source(&manifest, false).await;
    match result {
        Err(RegistryError::InvalidManifest { field, .. }) => {
            assert_eq!(field, "name");
        }
        other => panic!("unexpected result: {:?}", other),
    }
}

#[tokio::test]
async fn test_install_from_artifact_rejects_non_https_url() {
    let temp = tempfile::tempdir().expect("tempdir");
    let installer = RegistryInstaller::new(
        temp.path().to_path_buf(),
        temp.path().join("tools"),
        temp.path().join("channels"),
    );

    let manifest = test_manifest(
        "demo",
        "tools-src/demo",
        test_artifact(
            Some("http://github.com/nearai/ironclaw/releases/latest/download/demo.wasm"),
            None,
        ),
    );

    let result = installer.install_from_artifact(&manifest, false).await;
    match result {
        Err(RegistryError::InvalidManifest { field, .. }) => {
            assert_eq!(field, "artifacts.wasm32-wasip2.url");
        }
        other => panic!("unexpected result: {:?}", other),
    }
}

#[tokio::test]
async fn test_install_from_artifact_rejects_disallowed_host() {
    let temp = tempfile::tempdir().expect("tempdir");
    let installer = RegistryInstaller::new(
        temp.path().to_path_buf(),
        temp.path().join("tools"),
        temp.path().join("channels"),
    );

    let manifest = test_manifest(
        "demo",
        "tools-src/demo",
        test_artifact(Some("https://169.254.169.254/latest/meta-data"), None),
    );

    let result = installer.install_from_artifact(&manifest, false).await;
    match result {
        Err(RegistryError::InvalidManifest { field, .. }) => {
            assert_eq!(field, "artifacts.wasm32-wasip2.url");
        }
        other => panic!("unexpected result: {:?}", other),
    }
}

#[tokio::test]
async fn test_install_from_artifact_rejects_null_sha256() {
    let temp = tempfile::tempdir().expect("tempdir");
    let installer = RegistryInstaller::new(
        temp.path().to_path_buf(),
        temp.path().join("tools"),
        temp.path().join("channels"),
    );

    // Valid URL but no sha256 — should be rejected before any download attempt
    let manifest = test_manifest(
        "demo",
        "tools-src/demo",
        test_artifact(
            Some(
                "https://github.com/nearai/ironclaw/releases/latest/download/demo-wasm32-wasip2.tar.gz",
            ),
            None, // sha256 = null
        ),
    );

    let result = installer.install_from_artifact(&manifest, false).await;
    match result {
        Err(RegistryError::MissingChecksum { name }) => {
            assert_eq!(name, "demo");
        }
        other => panic!("unexpected result: {:?}", other),
    }
}

#[test]
fn test_should_attempt_source_fallback_policy() {
    let download = RegistryError::DownloadFailed {
        url: "https://github.com/nearai/ironclaw/releases/latest/download/demo.wasm".to_string(),
        reason: "http status 404".to_string(),
    };
    assert!(should_attempt_source_fallback(&download));

    let already = RegistryError::AlreadyInstalled {
        name: "demo".to_string(),
        path: PathBuf::from("/tmp/demo.wasm"),
    };
    assert!(!should_attempt_source_fallback(&already));

    let invalid = RegistryError::InvalidManifest {
        name: "demo".to_string(),
        field: "artifacts.wasm32-wasip2.url",
        reason: "host not allowed".to_string(),
    };
    assert!(!should_attempt_source_fallback(&invalid));

    // MissingChecksum SHOULD allow source fallback (bootstrapping)
    let missing = RegistryError::MissingChecksum {
        name: "demo".to_string(),
    };
    assert!(should_attempt_source_fallback(&missing));
}

#[test]
fn test_extract_tar_gz() {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    // Create a tar.gz in memory with test.wasm and test.capabilities.json
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut builder = Builder::new(&mut encoder);

        let wasm_data = b"\0asm\x01\x00\x00\x00";
        let mut header = tar::Header::new_gnu();
        header.set_size(wasm_data.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, "test.wasm", &wasm_data[..])
            .unwrap();

        let caps_data = br#"{"auth":null}"#;
        let mut header = tar::Header::new_gnu();
        header.set_size(caps_data.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, "test.capabilities.json", &caps_data[..])
            .unwrap();

        builder.finish().unwrap();
    }
    let gz_bytes = encoder.finish().unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let wasm_path = tmp.path().join("test.wasm");
    let caps_path = tmp.path().join("test.capabilities.json");

    let result = extract_tar_gz(&TarGzExtraction {
        bytes: &gz_bytes,
        name: "test",
        target_wasm: &wasm_path,
        target_caps: &caps_path,
        url: "test://url",
    })
    .unwrap();

    assert!(wasm_path.exists());
    assert!(caps_path.exists());
    assert!(result.has_capabilities);
}

#[tokio::test]
async fn test_install_from_source_rejects_wrong_prefix_for_channel() {
    let temp = tempfile::tempdir().expect("tempdir");
    let installer = RegistryInstaller::new(
        temp.path().to_path_buf(),
        temp.path().join("tools"),
        temp.path().join("channels"),
    );

    // Channel manifest with tools-src/ prefix should be rejected
    let manifest = test_manifest_with_kind(
        "telegram",
        "tools-src/telegram",
        None,
        ManifestKind::Channel,
    );

    let result = installer.install_from_source(&manifest, false).await;
    match result {
        Err(RegistryError::InvalidManifest { field, reason, .. }) => {
            assert_eq!(field, "source.dir");
            assert!(reason.contains("channels-src/"), "reason: {}", reason);
        }
        other => panic!("unexpected result: {:?}", other),
    }
}

#[tokio::test]
async fn test_install_from_source_accepts_correct_channel_prefix() {
    let temp = tempfile::tempdir().expect("tempdir");
    let installer = RegistryInstaller::new(
        temp.path().to_path_buf(),
        temp.path().join("tools"),
        temp.path().join("channels"),
    );

    // Channel manifest with channels-src/ prefix should pass validation
    // (will fail later because source dir doesn't exist, which is fine)
    let manifest = test_manifest_with_kind(
        "telegram",
        "channels-src/telegram",
        None,
        ManifestKind::Channel,
    );

    let result = installer.install_from_source(&manifest, false).await;
    match result {
        Err(RegistryError::ManifestRead { reason, .. }) => {
            assert!(
                reason.contains("source directory does not exist"),
                "reason: {}",
                reason
            );
        }
        other => panic!("unexpected result: {:?}", other),
    }
}

#[test]
fn test_extract_tar_gz_missing_wasm() {
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use tar::Builder;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut builder = Builder::new(&mut encoder);

        let data = b"not a wasm file";
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_cksum();
        builder
            .append_data(&mut header, "wrong.wasm", &data[..])
            .unwrap();
        builder.finish().unwrap();
    }
    let gz_bytes = encoder.finish().unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let result = extract_tar_gz(&TarGzExtraction {
        bytes: &gz_bytes,
        name: "test",
        target_wasm: &tmp.path().join("test.wasm"),
        target_caps: &tmp.path().join("test.capabilities.json"),
        url: "test://url",
    });

    assert!(result.is_err());
}

// Regression test for issue #439: ChecksumMismatch on a `releases/latest` URL
// must allow source-build fallback (moving-target URL, not a security concern),
// while a mismatch on a version-pinned URL must remain a hard block.
#[test]
fn test_source_fallback_on_latest_url_mismatch() {
    let latest_mismatch = RegistryError::ChecksumMismatch {
        url: "https://github.com/nearai/ironclaw/releases/latest/download/github-wasm32-wasip2.tar.gz".to_string(),
        expected_sha256: "aaa".to_string(),
        actual_sha256: "bbb".to_string(),
    };
    assert!(
        should_attempt_source_fallback(&latest_mismatch),
        "ChecksumMismatch on releases/latest URL should allow source fallback"
    );

    let pinned_mismatch = RegistryError::ChecksumMismatch {
        url: "https://github.com/nearai/ironclaw/releases/download/v0.7.0/github-0.2.0-wasm32-wasip2.tar.gz".to_string(),
        expected_sha256: "aaa".to_string(),
        actual_sha256: "bbb".to_string(),
    };
    assert!(
        !should_attempt_source_fallback(&pinned_mismatch),
        "ChecksumMismatch on version-pinned URL must remain a hard block"
    );
}
