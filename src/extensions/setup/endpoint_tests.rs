//! Behavioural tests for offline endpoint validation.

use proptest::prelude::*;
use rstest::rstest;

use super::endpoint::validate_endpoint_url;
use super::error::EndpointRejection;

#[rstest]
#[case("https://mail.example.com/jmap/", "https://mail.example.com/jmap/")]
#[case("https://example.com", "https://example.com/")]
#[case("https://example.com:8443/base", "https://example.com:8443/base")]
fn accepts_and_canonicalizes_public_https(#[case] raw: &str, #[case] canonical: &str) {
    let parsed = validate_endpoint_url(raw).expect("valid endpoint");

    assert_eq!(parsed.expose(), canonical);
}

#[rstest]
#[case("", EndpointRejection::Empty)]
#[case("   ", EndpointRejection::Empty)]
#[case("http://example.com", EndpointRejection::InsecureScheme)]
#[case("ftp://example.com", EndpointRejection::InsecureScheme)]
#[case("example.com/path", EndpointRejection::NotAbsolute)]
#[case(
    "https://user:pass@example.com",
    EndpointRejection::CredentialsNotAllowed
)]
#[case("https://user@example.com", EndpointRejection::CredentialsNotAllowed)]
#[case("https://example.com/?token=abc", EndpointRejection::QueryNotAllowed)]
#[case("https://example.com/#frag", EndpointRejection::FragmentNotAllowed)]
#[case("https://localhost/jmap", EndpointRejection::LocalHostname)]
#[case("https://mail.localhost/jmap", EndpointRejection::LocalHostname)]
#[case("https://printer.local/jmap", EndpointRejection::LocalHostname)]
#[case("https://service.internal/jmap", EndpointRejection::LocalHostname)]
#[case("https://intranet/jmap", EndpointRejection::LocalHostname)]
#[case("https://127.0.0.1/jmap", EndpointRejection::NonPublicAddress)]
#[case("https://10.1.2.3/jmap", EndpointRejection::NonPublicAddress)]
#[case("https://192.168.0.1/jmap", EndpointRejection::NonPublicAddress)]
#[case("https://169.254.169.254/latest", EndpointRejection::NonPublicAddress)]
#[case("https://2130706433/jmap", EndpointRejection::NonPublicAddress)]
#[case("https://0x7f.1/jmap", EndpointRejection::NonPublicAddress)]
#[case("https://[::1]/jmap", EndpointRejection::NonPublicAddress)]
#[case("https://[fe80::1]/jmap", EndpointRejection::NonPublicAddress)]
#[case("https://[fd00::1]/jmap", EndpointRejection::NonPublicAddress)]
#[case("https://example.com\\@evil.com", EndpointRejection::InvalidCharacter)]
fn rejects_unsafe_endpoints(#[case] raw: &str, #[case] expected: EndpointRejection) {
    assert_eq!(validate_endpoint_url(raw), Err(expected));
}

#[test]
fn control_characters_are_rejected() {
    assert_eq!(
        validate_endpoint_url("https://example.com/\u{0007}"),
        Err(EndpointRejection::InvalidCharacter)
    );
    assert_eq!(
        validate_endpoint_url("https://example.com/a b"),
        Err(EndpointRejection::InvalidCharacter)
    );
}

#[test]
fn overlong_endpoints_are_rejected() {
    let raw = format!("https://example.com/{}", "a".repeat(9000));

    assert_eq!(validate_endpoint_url(&raw), Err(EndpointRejection::TooLong));
}

#[test]
fn endpoint_url_debug_is_redacted() {
    let parsed = validate_endpoint_url("https://mail.example.com/secret-path").expect("valid");
    let rendered = format!("{parsed:?}");

    assert_eq!(rendered, "EndpointUrl([REDACTED])");
    assert!(!rendered.contains("example.com"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn canonicalization_is_idempotent(path in "[a-zA-Z0-9/_-]{0,40}") {
        let raw = format!("https://example.com/{}", path.trim_start_matches('/'));
        let Ok(first) = validate_endpoint_url(&raw) else {
            return Ok(());
        };
        let second = validate_endpoint_url(first.expose()).expect("canonical URL revalidates");

        prop_assert_eq!(first, second);
    }

    #[test]
    fn arbitrary_strings_never_panic(raw in ".*") {
        let outcome = std::panic::catch_unwind(|| validate_endpoint_url(&raw));
        prop_assert!(outcome.is_ok());
    }
}
