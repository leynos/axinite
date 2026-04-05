//! Tests for WorkerHttpClient construction and error handling.

use rstest::rstest;
use uuid::Uuid;

use crate::testing::credentials::TEST_BEARER_TOKEN;
use crate::worker::api::WorkerHttpClient;

/// Regression test: WorkerHttpClient::new succeeds with valid URLs.
///
/// This test verifies that the fallible constructor properly constructs
/// a WorkerHttpClient with valid URLs without panicking or using `unwrap`.
#[rstest]
#[case("http://localhost:50051")]
#[case("http://localhost:50051/")]
#[case("http://example.com")]
#[case("https://api.example.com")]
fn worker_http_client_new_succeeds_with_valid_url(#[case] url: &str) {
    let result = WorkerHttpClient::new(
        url.to_string(),
        Uuid::new_v4(),
        TEST_BEARER_TOKEN.to_string(),
    );

    assert!(
        result.is_ok(),
        "WorkerHttpClient::new should succeed with valid URL, got error"
    );

    let client = result.expect("client should be built");
    assert_eq!(client.orchestrator_url(), url.trim_end_matches('/'));
}

/// Regression test: Verify that the new() constructor returns Result and
/// can be properly constructed in test contexts.
#[test]
fn worker_http_client_new_returns_ok_for_test_token() {
    let job_id = Uuid::new_v4();
    let result = WorkerHttpClient::new(
        "http://host.docker.internal:50051".to_string(),
        job_id,
        TEST_BEARER_TOKEN.to_string(),
    );

    assert!(result.is_ok(), "expected Ok, got error");

    let client = result.expect("client should be built");
    assert_eq!(client.job_id(), job_id);
    assert_eq!(
        client.orchestrator_url(),
        "http://host.docker.internal:50051"
    );
}
