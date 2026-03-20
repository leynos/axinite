//! Tests for skill fetching and archive validation helpers.

use std::io::Write;
use std::net::{IpAddr, SocketAddr};

use flate2::Compression;
use flate2::write::DeflateEncoder;
use rstest::rstest;

use super::url_policy::{HttpsUrl, NormalizedDomain};
use super::{extract_skill_from_zip, is_private_ip, validate_fetch_url, validate_resolved_addrs};

type ZipEntryBuilder = fn(&str, &[u8]) -> Vec<u8>;

#[rstest]
#[case("https://192.168.1.1/skill.md", "private")]
#[case("https://127.0.0.1/skill.md", "private")]
#[case("https://localhost/skill.md", "internal hostname")]
#[case("https://localhost./skill.md", "internal hostname")]
#[case("https://169.254.169.254/latest/meta-data/", "private")]
#[case("https://metadata.google.internal/something", "internal hostname")]
#[case("file:///etc/passwd", "Only HTTPS")]
#[case("https://[::ffff:127.0.0.1]/skill.md", "private")]
#[case("https://[::1]/skill.md", "private")]
#[case("https://service.internal/api", "internal hostname")]
#[case("https://myhost.local/skill.md", "internal hostname")]
fn test_validate_fetch_url_rejects_invalid_targets(
    #[case] input_url: &str,
    #[case] expected_error_substring: &str,
) {
    let err = match HttpsUrl::try_from(input_url) {
        Ok(url) => validate_fetch_url(&url).expect_err("URL should be rejected"),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains(expected_error_substring),
        "expected error containing '{expected_error_substring}', got: {err}",
    );
}

#[rstest]
#[case("https://clawhub.ai/api/v1/download?slug=foo")]
#[case("https://github.com/repo/SKILL.md")]
#[case("https://clawhub.dev/api/v1/download?slug=foo")]
#[case("https://raw.githubusercontent.com/user/repo/main/SKILL.md")]
#[case("https://example.com/skills/deploy.md")]
#[case("https://[::ffff:8.8.8.8]/skill.md")]
fn test_validate_fetch_url_allows_public_https_targets(#[case] input_url: &str) {
    let https = HttpsUrl::try_from(input_url).expect("public HTTPS URL should parse");
    validate_fetch_url(&https).expect("public HTTPS URL should be accepted");
}

#[rstest]
#[case("example.com", "127.0.0.1:443,[::1]:443", false, Some("private"))]
#[case("example.com", "8.8.8.8:443,[2606:4700:4700::1111]:443", true, None)]
fn test_validate_resolved_addrs_cases(
    #[case] hostname: &str,
    #[case] addrs_csv: &str,
    #[case] expected_ok: bool,
    #[case] expected_error_substring: Option<&str>,
) {
    let host = NormalizedDomain::new(hostname);
    let addrs: Vec<SocketAddr> = addrs_csv
        .split(',')
        .map(|addr| {
            addr.parse::<SocketAddr>()
                .expect("socket address fixture should parse")
        })
        .collect();

    if expected_ok {
        validate_resolved_addrs(&host, &addrs)
            .expect("resolved public addresses should be accepted");
    } else {
        let err = validate_resolved_addrs(&host, &addrs)
            .expect_err("resolved private addresses should be rejected");
        assert!(
            err.to_string()
                .contains(expected_error_substring.expect("expected error text")),
            "expected error containing '{:?}', got: {err}",
            expected_error_substring,
        );
    }
}

#[test]
fn test_validate_resolved_addrs_rejects_empty_results() {
    let host = NormalizedDomain::new("example.com");
    let err = validate_resolved_addrs(&host, &Vec::<SocketAddr>::new())
        .expect_err("empty DNS resolution results should be rejected");
    assert!(
        err.to_string()
            .contains("DNS resolution returned no addresses"),
        "expected empty-resolution error, got: {err}",
    );
}

#[rstest]
#[case::stored(
    build_zip_entry_store as ZipEntryBuilder,
    b"---\nname: stored\n---\n# Stored\n"
)]
#[case::deflate(
    build_zip_entry_deflate as ZipEntryBuilder,
    b"---\nname: test\n---\n# Test Skill\n"
)]
fn test_extract_skill_from_zip_success_cases(
    #[case] builder: ZipEntryBuilder,
    #[case] content: &[u8],
) {
    let zip = builder("SKILL.md", content);
    let result =
        extract_skill_from_zip(&zip).expect("ZIP archive containing SKILL.md should extract");
    assert_eq!(
        result,
        std::str::from_utf8(content).expect("fixture content should be valid UTF-8"),
    );
}

#[test]
fn test_extract_skill_from_zip_missing_skill_md() {
    let zip = build_zip_entry_store("_meta.json", b"{}");
    let err = extract_skill_from_zip(&zip).expect_err("archive without SKILL.md should fail");
    assert!(err.to_string().contains("does not contain SKILL.md"));
}

/// Build a minimal ZIP local-file entry using the stored (no-compression)
/// method for tests.
///
/// The `file_name` and `content` lengths are encoded as little-endian `u16`
/// and `u32` fields in the local file header.
fn build_zip_entry_store_with_uncompressed(
    file_name: &str,
    content: &[u8],
    claimed_uncompressed: u32,
) -> Vec<u8> {
    let mut zip = Vec::new();
    zip.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
    zip.extend_from_slice(&[0x0A, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&(content.len() as u32).to_le_bytes());
    zip.extend_from_slice(&claimed_uncompressed.to_le_bytes());
    zip.extend_from_slice(&(file_name.len() as u16).to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(file_name.as_bytes());
    zip.extend_from_slice(content);
    zip
}

/// Build a minimal ZIP local-file entry using the stored (no-compression)
/// method for tests.
///
/// The `file_name` and `content` lengths are encoded as little-endian `u16`
/// and `u32` fields in the local file header.
fn build_zip_entry_store(file_name: &str, content: &[u8]) -> Vec<u8> {
    build_zip_entry_store_with_uncompressed(file_name, content, content.len() as u32)
}

/// Build a raw ZIP local-file-header entry using DEFLATE compression (method 8).
fn build_zip_entry_deflate(file_name: &str, content: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(content)
        .expect("deflate encoder should accept fixture content");
    let compressed = encoder
        .finish()
        .expect("deflate encoder should finish fixture archive");

    let mut zip = Vec::new();
    zip.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
    zip.extend_from_slice(&[0x14, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00]);
    zip.extend_from_slice(&[0x08, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    zip.extend_from_slice(&(content.len() as u32).to_le_bytes());
    zip.extend_from_slice(&(file_name.len() as u16).to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(file_name.as_bytes());
    zip.extend_from_slice(&compressed);
    zip
}

/// Build a stored ZIP entry where the declared uncompressed size differs from the
/// actual payload length (used to exercise size-limit checks).
fn build_zip_entry_store_oversized(
    file_name: &str,
    content: &[u8],
    claimed_uncompressed: u32,
) -> Vec<u8> {
    build_zip_entry_store_with_uncompressed(file_name, content, claimed_uncompressed)
}

#[test]
fn test_zip_extract_ignores_non_skill_entries() {
    let mut zip = Vec::new();
    zip.extend_from_slice(&build_zip_entry_store("README.md", b"# Readme"));
    zip.extend_from_slice(&build_zip_entry_store("src/main.rs", b"fn main() {}"));

    let err = extract_skill_from_zip(&zip).expect_err("archive without root SKILL.md should fail");
    assert!(
        err.to_string().contains("does not contain SKILL.md"),
        "Expected 'does not contain SKILL.md' error, got: {err}",
    );
}

#[rstest]
#[case("../../SKILL.md", "path traversal entry")]
#[case("subdir/SKILL.md", "nested path")]
fn test_zip_extract_non_root_skill_md_rejected(#[case] path: &str, #[case] label: &str) {
    let zip = build_zip_entry_store(path, b"---\nname: x\n---\n# X\n");
    let err = extract_skill_from_zip(&zip)
        .expect_err("non-root SKILL.md entries should not satisfy extraction");
    assert!(
        err.to_string().contains("does not contain SKILL.md"),
        "{label} should not match SKILL.md, got: {err}",
    );
}

#[test]
fn test_zip_extract_oversized_rejected() {
    let zip = build_zip_entry_store_oversized("SKILL.md", b"tiny", 2 * 1024 * 1024);
    let err =
        extract_skill_from_zip(&zip).expect_err("oversized ZIP entry should be rejected safely");
    assert!(
        err.to_string().contains("too large"),
        "Oversized entry should be rejected, got: {err}",
    );
}

#[test]
fn test_zip_extract_stored_size_mismatch_rejected() {
    let zip = build_zip_entry_store_oversized("SKILL.md", b"tiny", 12);
    let err = extract_skill_from_zip(&zip).expect_err("stored ZIP size mismatches should fail");
    assert!(
        err.to_string().contains("truncated"),
        "Expected a truncation error for a mismatched stored entry, got: {err}",
    );
}

#[rstest]
#[case("10.0.0.1", true)]
#[case("10.255.255.255", true)]
#[case("172.16.0.1", true)]
#[case("172.31.255.255", true)]
#[case("192.168.1.1", true)]
#[case("192.168.0.0", true)]
#[case("169.254.1.1", false)]
#[case("169.254.0.1", false)]
#[case("169.254.255.255", false)]
#[case("8.8.8.8", false)]
#[case("1.1.1.1", false)]
#[case("93.184.216.34", false)]
#[case("151.101.1.67", false)]
#[case("fc00::1", true)]
#[case("fd12:3456:789a::1", true)]
#[case("::1", false)]
#[case("fe80::1", false)]
#[case("2001:4860:4860::8888", false)]
fn test_is_private_ip_cases(#[case] ip_str: &str, #[case] expect_private: bool) {
    let ip: IpAddr = ip_str.parse().expect("IP fixture should parse");
    assert_eq!(
        is_private_ip(&ip),
        expect_private,
        "Expected is_private_ip({ip_str}) = {expect_private}",
    );
}
