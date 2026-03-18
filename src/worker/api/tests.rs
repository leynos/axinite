//! Tests for the worker HTTP client and its shared API type conversions.

use rstest::rstest;

use super::*;
use crate::testing::credentials::TEST_BEARER_TOKEN;
use uuid::Uuid;

#[rstest]
#[case("llm/complete")]
#[case("credentials")]
fn test_url_construction(#[case] path: &str) {
    let client = WorkerHttpClient::new(
        "http://host.docker.internal:50051".to_string(),
        Uuid::nil(),
        TEST_BEARER_TOKEN.to_string(),
    );

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
#[case("stop", FinishReason::Stop)]
#[case("length", FinishReason::Length)]
#[case("tool_use", FinishReason::ToolUse)]
#[case("tool_calls", FinishReason::ToolUse)]
#[case("content_filter", FinishReason::ContentFilter)]
#[case("unknown", FinishReason::Unknown)]
fn test_parse_finish_reason(#[case] input: &str, #[case] expected: FinishReason) {
    assert_eq!(parse_finish_reason(input), expected);
}

#[test]
fn test_job_description_deserialization() {
    let json = r#"{"title":"Test","description":"desc","project_dir":null}"#;
    let job: JobDescription = serde_json::from_str(json)
        .expect("failed to deserialize JobDescription in test_job_description_deserialization");
    assert_eq!(job.title, "Test");
    assert_eq!(job.description, "desc");
    assert!(job.project_dir.is_none());
}
