use std::sync::Arc;

use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::pairing::PairingStore;
use crate::testing::credentials::TEST_TELEGRAM_BOT_TOKEN;

use super::super::store::{ChannelStoreData, ResolvedHostCredential};
use super::super::types::{ChannelName, HostPattern, SecretValue};

#[test]
fn test_redact_credentials_replaces_values() {
    let mut creds = std::collections::HashMap::new();
    creds.insert(
        "TELEGRAM_BOT_TOKEN".to_string(),
        SecretValue::new(TEST_TELEGRAM_BOT_TOKEN.to_string()),
    );
    creds.insert("OTHER_SECRET".to_string(), SecretValue::new("s3cret"));
    let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

    let store = ChannelStoreData::new(
        1024 * 1024,
        &channel_name,
        ChannelCapabilities::default(),
        creds,
        Vec::new(),
        Arc::new(PairingStore::new()),
    );

    let error = format!(
        "HTTP request failed: error sending request for url \
            (https://api.telegram.org/bot{TEST_TELEGRAM_BOT_TOKEN}/getUpdates)"
    );

    let redacted = store.redact_credentials(&error);

    assert!(
        !redacted.contains(TEST_TELEGRAM_BOT_TOKEN),
        "credential value should be redacted"
    );
    assert!(
        redacted.contains("[REDACTED:TELEGRAM_BOT_TOKEN]"),
        "redacted text should contain placeholder name"
    );
    assert!(
        !redacted.contains("s3cret"),
        "other credentials should also be redacted"
    );
}

#[test]
fn test_redact_credentials_no_op_without_credentials() {
    let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

    let store = ChannelStoreData::new(
        1024 * 1024,
        &channel_name,
        ChannelCapabilities::default(),
        std::collections::HashMap::new(),
        Vec::new(),
        Arc::new(PairingStore::new()),
    );

    let input = "some error message";
    assert_eq!(store.redact_credentials(input), input);
}

#[test]
fn test_redact_credentials_url_encoded() {
    // Credential with characters that get URL-encoded
    let mut creds = std::collections::HashMap::new();
    creds.insert(
        "API_KEY".to_string(),
        SecretValue::new("key with spaces&special=chars"),
    );

    let host_creds = vec![ResolvedHostCredential {
        host_patterns: vec![
            HostPattern::new("api.example.com").expect("test host pattern is non-empty"),
        ],
        headers: std::collections::HashMap::new(),
        query_params: std::collections::HashMap::new(),
        secret_value: SecretValue::new("host secret+value"),
    }];
    let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

    let store = ChannelStoreData::new(
        1024 * 1024,
        &channel_name,
        ChannelCapabilities::default(),
        creds,
        host_creds,
        Arc::new(PairingStore::new()),
    );

    // Error containing URL-encoded form of the credential
    let error = "request failed: https://api.example.com?key=key%20with%20spaces%26special%3Dchars&host=host%20secret%2Bvalue";

    let redacted = store.redact_credentials(error);

    assert!(
        !redacted.contains("key%20with%20spaces"),
        "URL-encoded credential should be redacted, got: {}",
        redacted
    );
    assert!(
        !redacted.contains("host%20secret%2Bvalue"),
        "URL-encoded host credential should be redacted, got: {}",
        redacted
    );
}

#[test]
fn test_redact_credentials_skips_empty_values() {
    let mut creds = std::collections::HashMap::new();
    creds.insert("EMPTY_TOKEN".to_string(), SecretValue::new(String::new()));
    let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

    let store = ChannelStoreData::new(
        1024 * 1024,
        &channel_name,
        ChannelCapabilities::default(),
        creds,
        Vec::new(),
        Arc::new(PairingStore::new()),
    );

    let input = "should not match anything";
    assert_eq!(store.redact_credentials(input), input);
}

fn test_channel_http_capabilities(host: &str) -> ChannelCapabilities {
    use crate::tools::wasm::{Capabilities, EndpointPattern, HttpCapability};

    ChannelCapabilities::for_channel("test").with_tool_capabilities(
        Capabilities::default().with_http(HttpCapability::new(vec![
            EndpointPattern::host(host.to_string())
                .with_path_prefix("/")
                .with_methods(vec!["GET".to_string()]),
        ])),
    )
}

#[test]
fn test_channel_http_request_allows_placeholder_header_injection() {
    use crate::channels::wasm::wrapper::ChannelStoreData;
    use crate::channels::wasm::wrapper::near::agent::channel_host;
    use std::collections::HashMap;

    let host = "slack.invalid";
    let slack_bot_token = "slack-dummy-token-12345".to_string();
    let mut credentials = HashMap::new();
    credentials.insert(
        "SLACK_BOT_TOKEN".to_string(),
        SecretValue::new(slack_bot_token),
    );
    let channel_name = ChannelName::new("test").expect("test channel name is non-empty");

    let mut store = ChannelStoreData::new(
        1024 * 1024,
        &channel_name,
        test_channel_http_capabilities(host),
        credentials,
        Vec::new(),
        Arc::new(PairingStore::new()),
    );

    let err = <ChannelStoreData as channel_host::Host>::http_request(
        &mut store,
        channel_host::HttpRequestParams {
            method: "GET".to_string(),
            url: format!("https://{host}/api/chat.postMessage"),
            headers_json: serde_json::json!({
                "Authorization": "Bearer {SLACK_BOT_TOKEN}",
                "Content-Type": "application/json"
            })
            .to_string(),
            body: None,
            timeout_ms: Some(1000),
        },
    )
    .expect_err("invalid public hostname should fail after request preparation");

    assert!(
        !err.contains("Potential secret leak blocked"),
        "placeholder-based auth header should progress past leak scanning, got: {err}"
    );
    assert!(
        err.contains("HTTP request failed") || err.contains("dns error"),
        "expected later-stage HTTP/DNS failure, got: {err}"
    );
}
