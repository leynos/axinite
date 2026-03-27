//! Finish reason deserialisation and conversion tests.

use rstest::rstest;
use serde_json::json;

use crate::llm::FinishReason as LlmFinishReason;
use crate::worker::api::types::FinishReason as ProxyFinishReason;

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
fn test_proxy_finish_reason_unknown_provider_value_falls_back() {
    let reason = serde_json::from_value::<ProxyFinishReason>(json!("made_up_reason"))
        .expect("failed to deserialize unknown ProxyFinishReason as fallback");
    assert_eq!(reason, ProxyFinishReason::Unknown);
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
    let job: crate::worker::api::JobDescription = serde_json::from_str(json)
        .expect("failed to deserialize JobDescription in test_job_description_deserialization");
    assert_eq!(job.title, "Test");
    assert_eq!(job.description, "desc");
    assert!(job.project_dir.is_none());
}

#[test]
fn test_status_update_new_serializes_worker_state() {
    use crate::worker::api::{StatusUpdate, WorkerState};

    let update = StatusUpdate::new(WorkerState::Running, None, 0);
    let json = serde_json::to_string(&update).expect("failed to serialize StatusUpdate");
    let value: serde_json::Value =
        serde_json::from_str(&json).expect("failed to parse serialized StatusUpdate");
    assert_eq!(value["state"], "running");
}

#[test]
fn test_status_update_deserializes_worker_state() {
    use crate::worker::api::{StatusUpdate, WorkerState};

    let json = r#"{"state":"completed","message":"done","iteration":5}"#;
    let update: StatusUpdate =
        serde_json::from_str(json).expect("failed to deserialize StatusUpdate");
    assert_eq!(
        update.state,
        WorkerState::Completed,
        "\"state\":\"completed\" must map to WorkerState::Completed"
    );
    assert_eq!(update.message, Some("done".to_string()));
    assert_eq!(update.iteration, 5);
}
