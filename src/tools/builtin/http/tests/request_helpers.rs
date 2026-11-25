//! Tests for the request-building helpers: method dispatch and body attachment.

use reqwest::Client;

use crate::tools::builtin::http::request::{attach_body, request_builder};
use crate::tools::tool::ToolError;

fn url() -> reqwest::Url {
    reqwest::Url::parse("https://api.example.com/v1").expect("valid url")
}

#[test]
fn request_builder_accepts_supported_methods_in_any_case() {
    for (given, expected) in [
        ("get", "GET"),
        ("Post", "POST"),
        ("PUT", "PUT"),
        ("delete", "DELETE"),
        ("patch", "PATCH"),
    ] {
        let built = request_builder(&Client::new(), given, &url())
            .expect("supported method")
            .build()
            .expect("request builds");
        assert_eq!(built.method().as_str(), expected);
    }
}

#[test]
fn request_builder_rejects_unsupported_method() {
    let err = request_builder(&Client::new(), "TRACE", &url()).expect_err("unsupported");
    assert!(matches!(err, ToolError::InvalidParameters(_)));
}

fn attached(body: Option<&serde_json::Value>) -> (reqwest::Request, Option<Vec<u8>>) {
    let request = Client::new().post(url());
    let (request, bytes) = attach_body(request, body).expect("body attaches");
    (request.build().expect("request builds"), bytes)
}

#[test]
fn attach_body_without_a_body_returns_no_bytes() {
    let (built, bytes) = attached(None);
    assert!(built.body().is_none());
    assert!(bytes.is_none());
}

#[test]
fn attach_body_sends_a_json_string_as_json() {
    let (built, bytes) = attached(Some(&serde_json::json!("{\"a\":1}")));
    assert_eq!(bytes.as_deref(), Some(b"{\"a\":1}".as_slice()));
    assert_eq!(
        built
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/json")
    );
}

#[test]
fn attach_body_sends_a_plain_string_as_is() {
    let (built, bytes) = attached(Some(&serde_json::json!("not json")));
    assert_eq!(bytes.as_deref(), Some(b"not json".as_slice()));
    assert!(built.headers().get("content-type").is_none());
}

#[test]
fn attach_body_serializes_a_non_string_value() {
    let (_built, bytes) = attached(Some(&serde_json::json!({"a": 1})));
    assert_eq!(bytes.as_deref(), Some(b"{\"a\":1}".as_slice()));
}
