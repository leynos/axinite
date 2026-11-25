//! Tests for how injected registry credentials reach the request and the leak-scan inputs.

use reqwest::Client;

use crate::tools::builtin::http::request::{RequestTarget, apply_credentials};
use crate::tools::wasm::InjectedCredentials;

fn mixed_credentials() -> InjectedCredentials {
    let mut injected = InjectedCredentials::empty();
    injected
        .headers
        .insert("Authorization".to_string(), "Bearer sk-test".to_string());
    injected
        .query_params
        .insert("api_key".to_string(), "qp-secret".to_string());
    injected
}

#[test]
fn mixed_credentials_reach_request_and_leak_scan_inputs() {
    let mut url = reqwest::Url::parse("https://api.example.com/v1?existing=1").expect("valid url");
    let mut headers = vec![("X-Existing".to_string(), "keep".to_string())];
    let request = Client::new().get(url.clone());

    let request = apply_credentials(
        request,
        &mut RequestTarget {
            url: &mut url,
            headers: &mut headers,
        },
        &mixed_credentials(),
    );
    let built = request.build().expect("request builds");

    assert_eq!(
        built
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok()),
        Some("Bearer sk-test")
    );
    let sent: Vec<(String, String)> = built
        .url()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    assert!(sent.contains(&("existing".to_string(), "1".to_string())));
    assert!(sent.contains(&("api_key".to_string(), "qp-secret".to_string())));

    assert_eq!(
        headers,
        vec![
            ("X-Existing".to_string(), "keep".to_string()),
            ("Authorization".to_string(), "Bearer sk-test".to_string()),
        ]
    );
    assert!(
        url.query_pairs()
            .any(|(k, v)| k == "api_key" && v == "qp-secret")
    );
}

#[test]
fn empty_credentials_leave_request_and_target_untouched() {
    let mut url = reqwest::Url::parse("https://api.example.com/v1").expect("valid url");
    let mut headers = Vec::new();
    let request = Client::new().get(url.clone());

    let request = apply_credentials(
        request,
        &mut RequestTarget {
            url: &mut url,
            headers: &mut headers,
        },
        &InjectedCredentials::empty(),
    );
    let built = request.build().expect("request builds");

    assert!(built.headers().is_empty());
    assert_eq!(built.url().as_str(), "https://api.example.com/v1");
    assert!(headers.is_empty());
    assert_eq!(url.as_str(), "https://api.example.com/v1");
}
