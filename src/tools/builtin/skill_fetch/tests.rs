//! Tests for skill-fetch URL validation helpers.

use std::net::SocketAddr;

use rstest::rstest;

use super::url_policy::{HttpsUrl, NormalizedDomain, redact_url};
use super::{is_private_ip, validate_fetch_url, validate_resolved_addrs};

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

#[test]
fn test_validate_fetch_url_rejects_internal_host_without_leaking_credentials() {
    let https = HttpsUrl::try_from("https://user:secret@localhost/path?token=abc")
        .expect("credentialled localhost URL should parse");
    let err = validate_fetch_url(&https).expect_err("localhost URL should be rejected");
    let err_text = err.to_string();
    assert!(
        !err_text.contains("secret"),
        "validation error should not include credentials, got: {err_text}",
    );
    assert!(
        !err_text.contains("token=abc"),
        "validation error should not include query strings, got: {err_text}",
    );
}

#[test]
fn test_https_url_parse_error_does_not_echo_raw_input() {
    let raw_input = "not a url";
    let err = match HttpsUrl::try_from(raw_input) {
        Ok(_) => panic!("malformed URL should be rejected"),
        Err(err) => err,
    };
    let err_text = err.to_string();
    assert!(
        !err_text.contains(raw_input),
        "parse error should not echo the raw input, got: {err_text}",
    );
}

#[test]
fn test_redact_url_strips_userinfo_and_query() {
    let parsed =
        reqwest::Url::parse("https://user:secret@example.com/path/to/skill.zip?token=abc#fragment")
            .expect("fixture URL should parse");
    let redacted = redact_url(&parsed);
    assert_eq!(redacted, "https://example.com/path/to/skill.zip");
}

#[rstest]
#[case(IpAddrRepr::V4("10.0.0.1"), true)]
#[case(IpAddrRepr::V4("10.255.255.255"), true)]
#[case(IpAddrRepr::V4("172.16.0.1"), true)]
#[case(IpAddrRepr::V4("172.31.255.255"), true)]
#[case(IpAddrRepr::V4("192.168.0.0"), true)]
#[case(IpAddrRepr::V4("172.15.255.255"), false)]
#[case(IpAddrRepr::V4("172.32.0.0"), false)]
#[case(IpAddrRepr::V4("192.167.255.255"), false)]
#[case(IpAddrRepr::V4("192.169.0.0"), false)]
#[case(IpAddrRepr::V4("169.254.0.1"), false)]
#[case(IpAddrRepr::V4("8.8.8.8"), false)]
#[case(IpAddrRepr::V6("::1"), false)]
#[case(IpAddrRepr::V6("2001:4860:4860::8888"), false)]
fn test_is_private_ip_cases(#[case] ip_repr: IpAddrRepr, #[case] expected: bool) {
    assert_eq!(is_private_ip(&ip_repr.parse()), expected);
}

enum IpAddrRepr<'a> {
    V4(&'a str),
    V6(&'a str),
}

impl IpAddrRepr<'_> {
    fn parse(&self) -> std::net::IpAddr {
        match self {
            Self::V4(value) | Self::V6(value) => value.parse().expect("IP fixture should parse"),
        }
    }
}
