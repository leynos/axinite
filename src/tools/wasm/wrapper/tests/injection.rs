//! Tests for credential injection, redaction, leak-scan ordering, and
//! prepared HTTP request handling.

use crate::testing::credentials::TEST_BEARER_TOKEN_123;
use crate::tools::wasm::capabilities::Capabilities;

#[test]
fn test_inject_host_credentials_bearer() {
    use crate::tools::wasm::wrapper::{ResolvedHostCredential, StoreData};
    use std::collections::HashMap;

    let host_credentials = vec![ResolvedHostCredential {
        host_patterns: vec!["www.googleapis.com".to_string()],
        headers: {
            let mut h = HashMap::new();
            h.insert(
                "Authorization".to_string(),
                format!("Bearer {TEST_BEARER_TOKEN_123}"),
            );
            h
        },
        query_params: HashMap::new(),
        secret_value: TEST_BEARER_TOKEN_123.to_string(),
    }];

    let store_data = StoreData::new(
        1024 * 1024,
        Capabilities::default(),
        HashMap::new(),
        host_credentials,
    );

    // Should inject for matching host
    let mut headers = HashMap::new();
    let mut url = "https://www.googleapis.com/calendar/v3/events".to_string();
    store_data.inject_host_credentials("www.googleapis.com", &mut headers, &mut url);
    assert_eq!(
        headers.get("Authorization"),
        Some(&format!("Bearer {TEST_BEARER_TOKEN_123}"))
    );

    // Should not inject for non-matching host
    let mut headers2 = HashMap::new();
    let mut url2 = "https://other.com/api".to_string();
    store_data.inject_host_credentials("other.com", &mut headers2, &mut url2);
    assert!(!headers2.contains_key("Authorization"));
}

#[test]
fn test_inject_host_credentials_query_params() {
    use crate::tools::wasm::wrapper::{ResolvedHostCredential, StoreData};
    use std::collections::HashMap;

    let host_credentials = vec![ResolvedHostCredential {
        host_patterns: vec!["api.example.com".to_string()],
        headers: HashMap::new(),
        query_params: {
            let mut q = HashMap::new();
            q.insert("api_key".to_string(), "secret123".to_string());
            q
        },
        secret_value: "secret123".to_string(),
    }];

    let store_data = StoreData::new(
        1024 * 1024,
        Capabilities::default(),
        HashMap::new(),
        host_credentials,
    );

    let mut headers = HashMap::new();
    let mut url = "https://api.example.com/v1/data".to_string();
    store_data.inject_host_credentials("api.example.com", &mut headers, &mut url);
    assert!(url.contains("api_key=secret123"));
    assert!(url.contains('?'));
}

#[test]
fn test_redact_credentials_includes_host_credentials() {
    use crate::tools::wasm::wrapper::{ResolvedHostCredential, StoreData};
    use std::collections::HashMap;

    let host_credentials = vec![ResolvedHostCredential {
        host_patterns: vec!["api.example.com".to_string()],
        headers: HashMap::new(),
        query_params: HashMap::new(),
        secret_value: "super-secret-token".to_string(),
    }];

    let store_data = StoreData::new(
        1024 * 1024,
        Capabilities::default(),
        HashMap::new(),
        host_credentials,
    );

    let text = "Error: request to https://api.example.com?key=super-secret-token failed";
    let redacted = store_data.redact_credentials(text);
    assert!(!redacted.contains("super-secret-token"));
    assert!(redacted.contains("[REDACTED:host_credential]"));
}

#[test]
fn test_redact_credentials_includes_percent_encoded_host_credentials() {
    use crate::tools::wasm::wrapper::{ResolvedHostCredential, StoreData};
    use std::collections::HashMap;

    let host_credentials = vec![ResolvedHostCredential {
        host_patterns: vec!["api.example.com".to_string()],
        headers: HashMap::new(),
        query_params: HashMap::new(),
        secret_value: "super secret token".to_string(),
    }];

    let store_data = StoreData::new(
        1024 * 1024,
        Capabilities::default(),
        HashMap::new(),
        host_credentials,
    );

    let text = "Error: request to https://api.example.com?key=super%20secret%20token failed";
    let redacted = store_data.redact_credentials(text);
    assert!(!redacted.contains("super%20secret%20token"));
    assert!(redacted.contains("[REDACTED:host_credential]"));
}

fn test_github_pat() -> String {
    let prefix = "github";
    let marker = "_pat_";
    let account = "A".repeat(22);
    let separator = "_";
    let token = "B".repeat(59);
    format!("{prefix}{marker}{account}{separator}{token}")
}

fn test_prepare_request_capabilities(host: &str) -> Capabilities {
    use crate::tools::wasm::capabilities::{EndpointPattern, HttpCapability};

    Capabilities {
        http: Some(HttpCapability::new(vec![
            EndpointPattern::host(host.to_string())
                .with_path_prefix("/")
                .with_methods(vec!["GET".to_string()]),
        ])),
        ..Default::default()
    }
}

#[test]
fn test_prepare_http_request_allows_host_injected_github_pat() {
    use crate::tools::wasm::wrapper::{ResolvedHostCredential, StoreData};
    use std::collections::HashMap;

    let pat = test_github_pat();
    let host = "api.github.invalid";
    let host_credentials = vec![ResolvedHostCredential {
        host_patterns: vec![host.to_string()],
        headers: {
            let mut h = HashMap::new();
            h.insert("Authorization".to_string(), format!("Bearer {pat}"));
            h
        },
        query_params: HashMap::new(),
        secret_value: pat.clone(),
    }];

    let mut store_data = StoreData::new(
        1024 * 1024,
        test_prepare_request_capabilities(host),
        HashMap::new(),
        host_credentials,
    );

    let prepared = store_data
        .prepare_http_request(
            "GET",
            &format!("https://{host}/repos/leynos/mxd"),
            "{}",
            None,
        )
        .expect("host-injected PAT should not trip leak scanning");

    assert_eq!(
        prepared.headers.get("Authorization"),
        Some(&format!("Bearer {pat}"))
    );
    assert_eq!(prepared.url, format!("https://{host}/repos/leynos/mxd"));
}

#[test]
fn test_prepare_http_request_blocks_wasm_supplied_github_pat() {
    use crate::tools::wasm::wrapper::StoreData;
    use std::collections::HashMap;

    let pat = test_github_pat();
    let host = "api.github.invalid";
    let mut store_data = StoreData::new(
        1024 * 1024,
        test_prepare_request_capabilities(host),
        HashMap::new(),
        Vec::new(),
    );

    let headers_json = serde_json::json!({
        "Authorization": format!("Bearer {pat}")
    })
    .to_string();

    let err = match store_data.prepare_http_request(
        "GET",
        &format!("https://{host}/repos/leynos/mxd"),
        &headers_json,
        None,
    ) {
        Ok(_) => panic!("WASM-supplied PAT must still be blocked"),
        Err(err) => err,
    };

    assert!(err.contains("Potential secret leak blocked"));
    assert!(err.contains("header:Authorization"));
    assert!(err.contains("github_fine_grained_pat"));
}

#[test]
fn test_prepare_http_request_allows_placeholder_header_injection() {
    use crate::tools::wasm::wrapper::StoreData;
    use std::collections::HashMap;

    let host = "slack.invalid";
    let slack_bot_token = "slack-dummy-token-12345".to_string();
    let mut credentials = HashMap::new();
    credentials.insert("SLACK_BOT_TOKEN".to_string(), slack_bot_token.clone());

    let mut store_data = StoreData::new(
        1024 * 1024,
        test_prepare_request_capabilities(host),
        credentials,
        Vec::new(),
    );

    let headers_json = serde_json::json!({
        "Authorization": "Bearer {SLACK_BOT_TOKEN}",
        "Content-Type": "application/json"
    })
    .to_string();

    let prepared = store_data
        .prepare_http_request(
            "GET",
            &format!("https://{host}/api/chat.postMessage"),
            &headers_json,
            None,
        )
        .expect("placeholder-based auth header should pass leak scanning");

    assert_eq!(
        prepared.headers.get("Authorization"),
        Some(&format!("Bearer {slack_bot_token}"))
    );
}

#[test]
fn test_http_request_progresses_past_leak_scan_for_host_injected_github_pat() {
    use crate::tools::wasm::wrapper::near::agent::host;
    use crate::tools::wasm::wrapper::{ResolvedHostCredential, StoreData};
    use std::collections::HashMap;

    let pat = test_github_pat();
    let host = "api.github.invalid";
    let host_credentials = vec![ResolvedHostCredential {
        host_patterns: vec![host.to_string()],
        headers: {
            let mut h = HashMap::new();
            h.insert("Authorization".to_string(), format!("Bearer {pat}"));
            h
        },
        query_params: HashMap::new(),
        secret_value: pat,
    }];

    let mut store_data = StoreData::new(
        1024 * 1024,
        test_prepare_request_capabilities(host),
        HashMap::new(),
        host_credentials,
    );

    let err = <StoreData as host::Host>::http_request(
        &mut store_data,
        host::HttpRequestParams {
            method: "GET".to_string(),
            url: format!("https://{host}/repos/leynos/mxd"),
            headers_json: "{}".to_string(),
            body: None,
            timeout_ms: Some(1000),
        },
    )
    .expect_err("invalid public hostname should fail after request preparation");

    assert!(
        !err.contains("Potential secret leak blocked"),
        "request should progress past leak scanning, got: {err}"
    );
    assert!(
        err.contains("DNS resolution failed"),
        "expected later-stage DNS failure, got: {err}"
    );
}

/// Regression test: leak scan must run on raw headers (before credential
/// injection), not after. If it ran post-injection, the host-injected
/// Slack bot token would trigger a Block and reject the tool's own
/// legitimate outbound request.
#[test]
fn test_leak_scan_runs_before_credential_injection() {
    use crate::safety::LeakDetector;

    // Simulate pre-injection headers: WASM only sees the placeholder, not the real token.
    let raw_headers: Vec<(String, String)> = vec![
        (
            "Authorization".to_string(),
            "Bearer {SLACK_BOT_TOKEN}".to_string(),
        ),
        ("Content-Type".to_string(), "application/json".to_string()),
    ];

    let detector = LeakDetector::new();

    // Pre-injection scan should pass — placeholders are not secrets.
    let pre_result =
        detector.scan_http_request("https://slack.com/api/chat.postMessage", &raw_headers, None);
    assert!(
        pre_result.is_ok(),
        "Leak scan on pre-injection headers should pass, but got: {:?}",
        pre_result
    );

    // Post-injection headers would contain a real Slack token.
    let post_injection_token = ["xox", "b-", "1234567890-abcdefghij"].concat();
    let post_injection_headers: Vec<(String, String)> = vec![
        (
            "Authorization".to_string(),
            format!("Bearer {post_injection_token}"),
        ),
        ("Content-Type".to_string(), "application/json".to_string()),
    ];

    // Post-injection scan WOULD block — this is the false positive
    // that the pre-injection ordering prevents.
    let post_result = detector.scan_http_request(
        "https://slack.com/api/chat.postMessage",
        &post_injection_headers,
        None,
    );
    assert!(
        post_result.is_err(),
        "Leak scan on post-injection headers should block the Slack token"
    );
}
