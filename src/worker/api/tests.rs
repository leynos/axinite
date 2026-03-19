//! Tests for the worker HTTP client and its shared API type conversions.

use rstest::rstest;

use super::*;
use crate::llm::FinishReason as LlmFinishReason;
use crate::testing::credentials::TEST_BEARER_TOKEN;
use serde_json::json;
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
#[case(json!("stop"), ProxyFinishReason::Stop)]
#[case(json!("length"), ProxyFinishReason::Length)]
#[case(json!("tool_use"), ProxyFinishReason::ToolUse)]
#[case(json!("tool_calls"), ProxyFinishReason::ToolUse)]
#[case(json!("content_filter"), ProxyFinishReason::ContentFilter)]
#[case(json!("unknown"), ProxyFinishReason::Unknown)]
fn test_proxy_finish_reason_deserialization(
    #[case] input: serde_json::Value,
    #[case] expected: ProxyFinishReason,
) {
    let actual: ProxyFinishReason = serde_json::from_value(input).expect(
        "failed to deserialize ProxyFinishReason in test_proxy_finish_reason_deserialization",
    );
    assert_eq!(actual, expected);
}

#[test]
fn test_proxy_finish_reason_rejects_unknown_provider_value() {
    let err = serde_json::from_value::<ProxyFinishReason>(json!("made_up_reason"))
        .expect_err("unexpectedly deserialized an unknown ProxyFinishReason");
    assert!(err.is_data());
}

#[rstest]
#[case(ProxyFinishReason::Stop, LlmFinishReason::Stop)]
#[case(ProxyFinishReason::Length, LlmFinishReason::Length)]
#[case(ProxyFinishReason::ToolUse, LlmFinishReason::ToolUse)]
#[case(ProxyFinishReason::ContentFilter, LlmFinishReason::ContentFilter)]
#[case(ProxyFinishReason::Unknown, LlmFinishReason::Unknown)]
fn test_proxy_finish_reason_conversion(
    #[case] input: ProxyFinishReason,
    #[case] expected: LlmFinishReason,
) {
    assert_eq!(LlmFinishReason::from(input), expected);
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
