//! Tests for how injected registry credentials reach the request and the leak-scan inputs.

use std::sync::Arc;

use reqwest::{Client, Url};

use crate::secrets::{CreateSecretParams, CredentialLocation, CredentialMapping, SecretsStore};
use crate::testing::credentials::test_secrets_store;
use crate::tools::builtin::http::HttpTool;
use crate::tools::builtin::http::request::{RequestTarget, apply_credentials};
use crate::tools::wasm::{InjectedCredentials, SharedCredentialRegistry};

const USER: &str = "user-1";
const HOST: &str = "api.example.com";

fn header_credentials(name: &str, value: &str) -> InjectedCredentials {
    let mut injected = InjectedCredentials::empty();
    injected.headers.insert(name.to_string(), value.to_string());
    injected
}

fn mixed_credentials() -> InjectedCredentials {
    let mut injected = header_credentials("Authorization", "Bearer sk-test");
    injected
        .query_params
        .insert("api_key".to_string(), "qp-secret".to_string());
    injected
}

/// Build a tool whose registry maps `mappings` and whose store holds `secrets`.
async fn tool_with(mappings: Vec<CredentialMapping>, secrets: &[(&str, &str)]) -> HttpTool {
    let registry = Arc::new(SharedCredentialRegistry::new());
    registry.add_mappings(mappings);
    let store = test_secrets_store();
    for (name, value) in secrets {
        store
            .create(USER, CreateSecretParams::new(*name, *value))
            .await
            .expect("secret created");
    }
    HttpTool::new()
        .expect("http client")
        .with_credentials(registry, Arc::new(store))
}

/// Inject the registry credentials for `url`, returning the built request and the mirrored target.
async fn inject(tool: &HttpTool, url: &str) -> (reqwest::Request, Url, Vec<(String, String)>) {
    let mut url = Url::parse(url).expect("valid url");
    let mut headers = Vec::new();
    let request = Client::new().get(url.clone());
    let request = tool
        .inject_registry_credentials(
            USER,
            request,
            &mut RequestTarget {
                url: &mut url,
                headers: &mut headers,
            },
        )
        .await;
    (request.build().expect("request builds"), url, headers)
}

fn header_values(request: &reqwest::Request, name: &str) -> Vec<String> {
    request
        .headers()
        .get_all(name)
        .iter()
        .map(|v| v.to_str().expect("ascii header").to_string())
        .collect()
}

#[test]
fn mixed_credentials_reach_request_and_leak_scan_inputs() {
    let mut url = Url::parse("https://api.example.com/v1?existing=1").expect("valid url");
    let mut headers = vec![("X-Existing".to_string(), "keep".to_string())];
    let request = Client::new().get(url.clone());

    let request = apply_credentials(
        request,
        &mut RequestTarget {
            url: &mut url,
            headers: &mut headers,
        },
        &[mixed_credentials()],
    );
    let built = request.build().expect("request builds");

    assert_eq!(header_values(&built, "Authorization"), ["Bearer sk-test"]);
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
    let mut url = Url::parse("https://api.example.com/v1").expect("valid url");
    let mut headers = Vec::new();
    let request = Client::new().get(url.clone());

    let request = apply_credentials(
        request,
        &mut RequestTarget {
            url: &mut url,
            headers: &mut headers,
        },
        &[],
    );
    let built = request.build().expect("request builds");

    assert!(built.headers().is_empty());
    assert_eq!(built.url().as_str(), "https://api.example.com/v1");
    assert!(headers.is_empty());
    assert_eq!(url.as_str(), "https://api.example.com/v1");
}

#[tokio::test]
async fn two_mappings_naming_one_header_both_reach_the_request() {
    let tool = tool_with(
        vec![
            CredentialMapping::header("key_a", "X-Api-Key", HOST),
            CredentialMapping::header("key_b", "X-Api-Key", HOST),
        ],
        &[("key_a", "value-a"), ("key_b", "value-b")],
    )
    .await;

    let (built, _url, mirrored) = inject(&tool, "https://api.example.com/v1").await;

    let mut sent = header_values(&built, "X-Api-Key");
    sent.sort();
    assert_eq!(sent, ["value-a", "value-b"]);
    let mut leak_scanned: Vec<&str> = mirrored.iter().map(|(_, v)| v.as_str()).collect();
    leak_scanned.sort_unstable();
    assert_eq!(leak_scanned, ["value-a", "value-b"]);
}

fn query_mapping(secret: &str, param: &str) -> CredentialMapping {
    CredentialMapping {
        secret_name: secret.to_string(),
        location: CredentialLocation::QueryParam {
            name: param.to_string(),
        },
        host_patterns: vec![HOST.to_string()],
    }
}

#[tokio::test]
async fn two_mappings_naming_one_query_parameter_both_reach_the_request_in_order() {
    let tool = tool_with(
        vec![
            query_mapping("key_a", "api_key"),
            query_mapping("key_b", "api_key"),
        ],
        &[("key_a", "value-a"), ("key_b", "value-b")],
    )
    .await;

    let (built, mirrored_url, _headers) = inject(&tool, "https://api.example.com/v1").await;

    let values = |url: &Url| -> Vec<String> {
        url.query_pairs()
            .filter(|(k, _)| k == "api_key")
            .map(|(_, v)| v.into_owned())
            .collect()
    };
    assert_eq!(values(built.url()), ["value-a", "value-b"]);
    assert_eq!(values(&mirrored_url), ["value-a", "value-b"]);
}

#[tokio::test]
async fn failed_secret_retrieval_skips_that_mapping_only() {
    let tool = tool_with(
        vec![
            CredentialMapping::header("missing_key", "X-Missing", HOST),
            CredentialMapping::header("present_key", "X-Present", HOST),
        ],
        &[("present_key", "present-value")],
    )
    .await;

    let (built, _url, mirrored) = inject(&tool, "https://api.example.com/v1").await;

    assert!(header_values(&built, "X-Missing").is_empty());
    assert_eq!(header_values(&built, "X-Present"), ["present-value"]);
    assert_eq!(
        mirrored,
        vec![("X-Present".to_string(), "present-value".to_string())]
    );
}

#[tokio::test]
async fn unmapped_host_and_missing_registry_inject_nothing() {
    let mapped = tool_with(
        vec![CredentialMapping::bearer("key_a", "other.example.com")],
        &[("key_a", "value-a")],
    )
    .await;
    let (built, _url, mirrored) = inject(&mapped, "https://api.example.com/v1").await;
    assert!(built.headers().is_empty());
    assert!(mirrored.is_empty());

    let bare = HttpTool::new().expect("http client");
    let (built, _url, mirrored) = inject(&bare, "https://api.example.com/v1").await;
    assert!(built.headers().is_empty());
    assert!(mirrored.is_empty());
}
