//! Tests for which webhook secret wins when a request carries several.

use std::collections::HashMap;

use axum::http::{HeaderMap, HeaderValue};

use crate::channels::wasm::router::webhook::provided_secret;

fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.insert(*name, HeaderValue::from_static(value));
    }
    map
}

fn query(secret: Option<&str>) -> HashMap<String, String> {
    secret
        .map(|s| HashMap::from([("secret".to_string(), s.to_string())]))
        .unwrap_or_default()
}

#[test]
fn query_parameter_wins_over_headers() {
    let h = headers(&[
        ("x-custom-secret", "from-header"),
        ("x-webhook-secret", "generic"),
    ]);
    assert_eq!(
        provided_secret(&query(Some("from-query")), &h, "x-custom-secret").as_deref(),
        Some("from-query")
    );
}

#[test]
fn configured_header_wins_over_the_generic_header() {
    let h = headers(&[
        ("x-custom-secret", "from-header"),
        ("x-webhook-secret", "generic"),
    ]);
    assert_eq!(
        provided_secret(&query(None), &h, "x-custom-secret").as_deref(),
        Some("from-header")
    );
}

#[test]
fn generic_header_is_the_fallback_when_the_configured_one_is_absent() {
    let h = headers(&[("x-webhook-secret", "generic")]);
    assert_eq!(
        provided_secret(&query(None), &h, "x-custom-secret").as_deref(),
        Some("generic")
    );
}

#[test]
fn no_secret_anywhere_yields_none() {
    assert_eq!(
        provided_secret(&query(None), &HeaderMap::new(), "x-custom-secret"),
        None
    );
}
