//! Unit tests for the sandbox HTTP proxy lifecycle and policy
//! enforcement.

use std::sync::Arc;

use hyper::StatusCode;

use super::handlers::{empty_body, error_response, is_hop_by_hop_header, make_response};
use super::{HttpProxy, NoCredentialResolver};
use crate::sandbox::proxy::allowlist::DomainAllowlist;
use crate::sandbox::proxy::policy::DefaultPolicyDecider;

#[tokio::test]
async fn test_proxy_starts_and_stops() {
    let allowlist = DomainAllowlist::new(&["example.com".to_string()]);
    let decider = Arc::new(DefaultPolicyDecider::new(allowlist, vec![]));
    let resolver = Arc::new(NoCredentialResolver);

    let proxy = HttpProxy::new(decider, resolver);

    let addr = proxy.start(0).await.unwrap();
    assert!(proxy.is_running());
    assert!(addr.port() > 0);

    proxy.stop().await;
    // Give it a moment to shut down
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
}

#[test]
fn test_hop_by_hop_headers() {
    assert!(is_hop_by_hop_header("connection"));
    assert!(is_hop_by_hop_header("Connection"));
    assert!(is_hop_by_hop_header("transfer-encoding"));
    assert!(!is_hop_by_hop_header("content-type"));
    assert!(!is_hop_by_hop_header("authorization"));
}

#[test]
fn test_make_response_does_not_panic() {
    let resp = make_response(StatusCode::OK, empty_body());
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = error_response(StatusCode::FORBIDDEN, "denied".to_string());
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
