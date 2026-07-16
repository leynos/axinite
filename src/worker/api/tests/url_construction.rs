//! URL construction and routing tests.

use rstest::rstest;
use uuid::Uuid;

use crate::testing::credentials::TEST_BEARER_TOKEN;
use crate::worker::api::{REMOTE_TOOL_CATALOG_PATH, REMOTE_TOOL_CATALOG_ROUTE, WorkerHttpClient};

#[rstest]
#[case("llm/complete")]
#[case("credentials")]
fn test_url_construction(#[case] path: &str) {
    let client = WorkerHttpClient::new(
        "http://host.docker.internal:50051".to_string(),
        Uuid::nil(),
        TEST_BEARER_TOKEN.to_string(),
    )
    .expect("test client should build");

    assert_eq!(
        client.url(path),
        format!(
            "http://host.docker.internal:50051/worker/{}/{}",
            Uuid::nil(),
            path
        )
    );
}

#[rstest]
#[case("llm/complete")]
#[case("credentials")]
fn test_url_construction_with_trailing_slash(#[case] path: &str) {
    let client = WorkerHttpClient::new(
        "http://host.docker.internal:50051/".to_string(),
        Uuid::nil(),
        TEST_BEARER_TOKEN.to_string(),
    )
    .expect("test client should build");

    assert_eq!(
        client.url(path),
        format!(
            "http://host.docker.internal:50051/worker/{}/{}",
            Uuid::nil(),
            path
        ),
        "trailing slash in base URL must be normalised away"
    );
}

#[test]
fn remote_tool_catalog_url_construction() {
    let client = WorkerHttpClient::new(
        "http://host.docker.internal:50051".to_string(),
        Uuid::nil(),
        TEST_BEARER_TOKEN.to_string(),
    )
    .expect("test client should build");

    assert_eq!(
        client.url(REMOTE_TOOL_CATALOG_PATH),
        format!(
            "http://host.docker.internal:50051{}",
            REMOTE_TOOL_CATALOG_ROUTE.replace("{job_id}", &Uuid::nil().to_string())
        )
    );
}
