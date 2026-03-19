//! Tests for skill fetching and archive validation helpers.

use std::net::{IpAddr, SocketAddr};

use flate2::Compression;
use flate2::write::DeflateEncoder;
use std::io::Write;

use super::{extract_skill_from_zip, is_private_ip, validate_fetch_url, validate_resolved_addrs};

#[test]
fn test_validate_fetch_url_allows_https() {
    assert!(validate_fetch_url("https://clawhub.ai/api/v1/download?slug=foo").is_ok());
}

#[test]
fn test_validate_fetch_url_rejects_http() {
    let err = validate_fetch_url("http://example.com/skill.md").unwrap_err();
    assert!(err.to_string().contains("Only HTTPS"));
}

#[test]
fn test_validate_fetch_url_rejects_private_ip() {
    let err = validate_fetch_url("https://192.168.1.1/skill.md").unwrap_err();
    assert!(err.to_string().contains("private"));
}

#[test]
fn test_validate_fetch_url_rejects_loopback() {
    let err = validate_fetch_url("https://127.0.0.1/skill.md").unwrap_err();
    assert!(err.to_string().contains("private"));
}

#[test]
fn test_validate_fetch_url_rejects_localhost() {
    let err = validate_fetch_url("https://localhost/skill.md").unwrap_err();
    assert!(err.to_string().contains("internal hostname"));
}

#[test]
fn test_validate_fetch_url_rejects_localhost_fqdn() {
    let err = validate_fetch_url("https://localhost./skill.md").unwrap_err();
    assert!(err.to_string().contains("internal hostname"));
}

#[test]
fn test_validate_fetch_url_rejects_metadata_endpoint() {
    let err = validate_fetch_url("https://169.254.169.254/latest/meta-data/").unwrap_err();
    assert!(err.to_string().contains("private"));
}

#[test]
fn test_validate_fetch_url_rejects_internal_domain() {
    let err = validate_fetch_url("https://metadata.google.internal/something").unwrap_err();
    assert!(err.to_string().contains("internal hostname"));
}

#[test]
fn test_validate_fetch_url_rejects_file_scheme() {
    let err = validate_fetch_url("file:///etc/passwd").unwrap_err();
    assert!(err.to_string().contains("Only HTTPS"));
}

#[test]
fn test_validate_fetch_url_rejects_ipv4_mapped_ipv6_loopback() {
    let err = validate_fetch_url("https://[::ffff:127.0.0.1]/skill.md").unwrap_err();
    assert!(err.to_string().contains("private") || err.to_string().contains("loopback"));
}

#[test]
fn test_validate_fetch_url_rejects_ipv6_loopback() {
    let err = validate_fetch_url("https://[::1]/skill.md").unwrap_err();
    assert!(err.to_string().contains("private") || err.to_string().contains("loopback"));
}

#[test]
fn test_validate_resolved_addrs_rejects_loopback_hostname() {
    let addrs = vec![
        "127.0.0.1:443".parse::<SocketAddr>().unwrap(),
        "[::1]:443".parse::<SocketAddr>().unwrap(),
    ];

    let err = validate_resolved_addrs("example.com", &addrs).unwrap_err();
    assert!(err.to_string().contains("private") || err.to_string().contains("loopback"));
}

#[test]
fn test_validate_resolved_addrs_allows_public_hostname() {
    let addrs = vec![
        "8.8.8.8:443".parse::<SocketAddr>().unwrap(),
        "[2606:4700:4700::1111]:443".parse::<SocketAddr>().unwrap(),
    ];

    assert!(validate_resolved_addrs("example.com", &addrs).is_ok());
}

#[test]
fn test_extract_skill_from_zip_deflate() {
    let skill_md = b"---\nname: test\n---\n# Test Skill\n";
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(skill_md).unwrap();
    let compressed = encoder.finish().unwrap();

    let mut zip = Vec::new();
    zip.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
    zip.extend_from_slice(&[0x14, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00]);
    zip.extend_from_slice(&[0x08, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
    zip.extend_from_slice(&(skill_md.len() as u32).to_le_bytes());
    zip.extend_from_slice(&8u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(b"SKILL.md");
    zip.extend_from_slice(&compressed);

    let result = extract_skill_from_zip(&zip).unwrap();
    assert_eq!(result, "---\nname: test\n---\n# Test Skill\n");
}

#[test]
fn test_extract_skill_from_zip_store() {
    let skill_md = b"---\nname: stored\n---\n# Stored\n";

    let mut zip = Vec::new();
    zip.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
    zip.extend_from_slice(&[0x0A, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&(skill_md.len() as u32).to_le_bytes());
    zip.extend_from_slice(&(skill_md.len() as u32).to_le_bytes());
    zip.extend_from_slice(&8u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(b"SKILL.md");
    zip.extend_from_slice(skill_md);

    let result = extract_skill_from_zip(&zip).unwrap();
    assert_eq!(result, "---\nname: stored\n---\n# Stored\n");
}

#[test]
fn test_extract_skill_from_zip_missing_skill_md() {
    let mut zip = Vec::new();
    zip.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
    zip.extend_from_slice(&[0x0A, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&2u32.to_le_bytes());
    zip.extend_from_slice(&2u32.to_le_bytes());
    zip.extend_from_slice(&10u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(b"_meta.json");
    zip.extend_from_slice(b"{}");

    let err = extract_skill_from_zip(&zip).unwrap_err();
    assert!(err.to_string().contains("does not contain SKILL.md"));
}

fn build_zip_entry_store(file_name: &str, content: &[u8]) -> Vec<u8> {
    let mut zip = Vec::new();
    zip.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
    zip.extend_from_slice(&[0x0A, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&(content.len() as u32).to_le_bytes());
    zip.extend_from_slice(&(content.len() as u32).to_le_bytes());
    zip.extend_from_slice(&(file_name.len() as u16).to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(file_name.as_bytes());
    zip.extend_from_slice(content);
    zip
}

#[test]
fn test_zip_extract_valid_skill() {
    let content = b"---\nname: hello\n---\n# Hello Skill\nDoes things.\n";
    let zip = build_zip_entry_store("SKILL.md", content);
    let result = extract_skill_from_zip(&zip).unwrap();
    assert_eq!(result, std::str::from_utf8(content).unwrap());
}

#[test]
fn test_zip_extract_ignores_non_skill_entries() {
    let mut zip = Vec::new();
    zip.extend_from_slice(&build_zip_entry_store("README.md", b"# Readme"));
    zip.extend_from_slice(&build_zip_entry_store("src/main.rs", b"fn main() {}"));

    let err = extract_skill_from_zip(&zip).unwrap_err();
    assert!(
        err.to_string().contains("does not contain SKILL.md"),
        "Expected 'does not contain SKILL.md' error, got: {}",
        err
    );
}

#[test]
fn test_zip_extract_path_traversal_rejected() {
    let content = b"---\nname: evil\n---\n# Malicious path traversal\n";
    let zip = build_zip_entry_store("../../SKILL.md", content);

    let err = extract_skill_from_zip(&zip).unwrap_err();
    assert!(
        err.to_string().contains("does not contain SKILL.md"),
        "Path traversal entry should not match SKILL.md, got: {}",
        err
    );
}

#[test]
fn test_zip_extract_nested_path_not_matched() {
    let content = b"---\nname: nested\n---\n# Nested\n";
    let zip = build_zip_entry_store("subdir/SKILL.md", content);

    let err = extract_skill_from_zip(&zip).unwrap_err();
    assert!(
        err.to_string().contains("does not contain SKILL.md"),
        "Nested path should not match SKILL.md, got: {}",
        err
    );
}

#[test]
fn test_zip_extract_oversized_rejected() {
    let oversized_claim: u32 = 2 * 1024 * 1024;
    let small_body = b"tiny";

    let mut zip = Vec::new();
    zip.extend_from_slice(&[0x50, 0x4B, 0x03, 0x04]);
    zip.extend_from_slice(&[0x0A, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    zip.extend_from_slice(&(small_body.len() as u32).to_le_bytes());
    zip.extend_from_slice(&oversized_claim.to_le_bytes());
    zip.extend_from_slice(&8u16.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes());
    zip.extend_from_slice(b"SKILL.md");
    zip.extend_from_slice(small_body);

    let err = extract_skill_from_zip(&zip).unwrap_err();
    assert!(
        err.to_string().contains("too large"),
        "Oversized entry should be rejected, got: {}",
        err
    );
}

#[test]
fn test_is_private_ip_blocks_loopback() {
    let loopback: IpAddr = "127.0.0.1".parse().unwrap();
    assert!(loopback.is_loopback());
    assert!(validate_fetch_url("https://127.0.0.1/skill.md").is_err());
}

#[test]
fn test_is_private_ip_blocks_private_ranges() {
    let cases: Vec<(&str, bool)> = vec![
        ("10.0.0.1", true),
        ("10.255.255.255", true),
        ("172.16.0.1", true),
        ("172.31.255.255", true),
        ("192.168.1.1", true),
        ("192.168.0.0", true),
    ];
    for (ip_str, expect_private) in cases {
        let ip: IpAddr = ip_str.parse().unwrap();
        assert_eq!(
            is_private_ip(&ip),
            expect_private,
            "Expected is_private_ip({}) = {}",
            ip_str,
            expect_private
        );
    }
}

#[test]
fn test_is_private_ip_blocks_link_local() {
    let cases = vec!["169.254.1.1", "169.254.0.1", "169.254.255.255"];
    for ip_str in cases {
        let ip: IpAddr = ip_str.parse().unwrap();
        assert!(
            is_private_ip(&ip),
            "Expected is_private_ip({}) = true (link-local)",
            ip_str
        );
    }
}

#[test]
fn test_is_private_ip_allows_public() {
    let public_ips = vec!["8.8.8.8", "1.1.1.1", "93.184.216.34", "151.101.1.67"];
    for ip_str in public_ips {
        let ip: IpAddr = ip_str.parse().unwrap();
        assert!(
            !is_private_ip(&ip),
            "Expected is_private_ip({}) = false (public IP)",
            ip_str
        );
        assert!(!ip.is_loopback(), "Expected {} is not loopback", ip_str);
    }
}

#[test]
fn test_is_private_ip_blocks_ipv4_mapped_ipv6() {
    let err = validate_fetch_url("https://[::ffff:127.0.0.1]/skill.md").unwrap_err();
    assert!(
        err.to_string().contains("private") || err.to_string().contains("loopback"),
        "IPv4-mapped loopback should be blocked, got: {}",
        err
    );

    let err = validate_fetch_url("https://[::ffff:192.168.1.1]/skill.md").unwrap_err();
    assert!(
        err.to_string().contains("private") || err.to_string().contains("loopback"),
        "IPv4-mapped private should be blocked, got: {}",
        err
    );

    let err = validate_fetch_url("https://[::ffff:10.0.0.1]/skill.md").unwrap_err();
    assert!(
        err.to_string().contains("private") || err.to_string().contains("loopback"),
        "IPv4-mapped 10.x should be blocked, got: {}",
        err
    );

    assert!(
        validate_fetch_url("https://[::ffff:8.8.8.8]/skill.md").is_ok(),
        "IPv4-mapped public IP should be allowed"
    );

    let err = validate_fetch_url("https://[::1]/skill.md").unwrap_err();
    assert!(
        err.to_string().contains("private") || err.to_string().contains("loopback"),
        "IPv6 loopback should be blocked, got: {}",
        err
    );
}

#[test]
fn test_is_restricted_host_blocks_metadata() {
    let err = validate_fetch_url("https://169.254.169.254/latest/meta-data/").unwrap_err();
    assert!(
        err.to_string().contains("private") || err.to_string().contains("link-local"),
        "Metadata IP should be blocked, got: {}",
        err
    );

    let err = validate_fetch_url("https://metadata.google.internal/something").unwrap_err();
    assert!(
        err.to_string().contains("internal hostname"),
        "metadata.google.internal should be blocked, got: {}",
        err
    );

    let err = validate_fetch_url("https://service.internal/api").unwrap_err();
    assert!(
        err.to_string().contains("internal hostname"),
        ".internal domains should be blocked, got: {}",
        err
    );

    let err = validate_fetch_url("https://myhost.local/skill.md").unwrap_err();
    assert!(
        err.to_string().contains("internal hostname"),
        ".local domains should be blocked, got: {}",
        err
    );
}

#[test]
fn test_is_restricted_host_allows_normal() {
    let allowed = vec![
        "https://github.com/repo/SKILL.md",
        "https://clawhub.dev/api/v1/download?slug=foo",
        "https://raw.githubusercontent.com/user/repo/main/SKILL.md",
        "https://example.com/skills/deploy.md",
    ];
    for url in allowed {
        assert!(
            validate_fetch_url(url).is_ok(),
            "Expected validate_fetch_url({}) to succeed",
            url
        );
    }
}
