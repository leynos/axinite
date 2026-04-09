//! JSON shape symmetry tests for worker-orchestrator wire types.
//!
//! Each test round-trips a DTO through JSON serialization and asserts the
//! wire shape via `insta` snapshot macros, so changes produce a single
//! diffable artifact.

use ironclaw::llm::ChatMessage;
use ironclaw::worker::api::{
    CompletionReport, CredentialResponse, JobDescription, JobEventPayload, JobEventType,
    PromptResponse, ProxyCompletionResponse, ProxyFinishReason, ProxyToolCompletionRequest,
    RemoteToolCatalogResponse, RemoteToolExecutionRequest, StatusUpdate, WorkerState,
};

#[test]
fn status_update_round_trips() {
    let original = StatusUpdate::new(WorkerState::InProgress, Some("working".into()), 42);
    let json = serde_json::to_string(&original).expect("serialize");
    insta::assert_json_snapshot!("status_update", &original);
    let back: StatusUpdate = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.state, original.state);
    assert_eq!(back.message, original.message);
    assert_eq!(back.iteration, original.iteration);
}

#[test]
fn job_event_payload_round_trips() {
    let original = JobEventPayload {
        event_type: JobEventType::ToolUse,
        data: serde_json::json!({"tool": "bash"}),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    insta::assert_json_snapshot!("job_event_payload", &original);
    let back: JobEventPayload = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.event_type, original.event_type);
    assert_eq!(back.data, original.data);
}

#[test]
fn completion_report_round_trips() {
    let original = CompletionReport {
        success: true,
        message: Some("done".into()),
        iterations: 10,
    };
    let json = serde_json::to_string(&original).expect("serialize");
    insta::assert_json_snapshot!("completion_report", &original);
    let back: CompletionReport = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.success, original.success);
    assert_eq!(back.message, original.message);
    assert_eq!(back.iterations, original.iterations);
}

#[test]
fn remote_tool_execution_request_round_trips() {
    let original = RemoteToolExecutionRequest {
        tool_name: "my_tool".into(),
        params: serde_json::json!({"key": "value"}),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    insta::assert_json_snapshot!("remote_tool_execution_request", &original);
    let back: RemoteToolExecutionRequest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, original);
}

#[test]
fn proxy_tool_completion_request_round_trips() {
    let original = ProxyToolCompletionRequest {
        messages: vec![ChatMessage::user("hello")],
        tools: vec![],
        model: None,
        max_tokens: None,
        temperature: None,
        tool_choice: Some("auto".into()),
    };
    let json = serde_json::to_string(&original).expect("serialize");
    insta::assert_json_snapshot!("proxy_tool_completion_request", &original);
    let back: ProxyToolCompletionRequest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.tool_choice, original.tool_choice);
}

#[test]
fn proxy_completion_response_from_fixture() {
    let fixture = serde_json::json!({
        "content": "Hello",
        "input_tokens": 100,
        "output_tokens": 50,
        "finish_reason": "stop",
        "cache_read_input_tokens": 10,
        "cache_creation_input_tokens": 5
    });
    let parsed: ProxyCompletionResponse = serde_json::from_value(fixture).expect("parse");
    insta::assert_json_snapshot!("proxy_completion_response", &parsed);
    assert_eq!(parsed.content, "Hello");
    assert_eq!(parsed.input_tokens, 100);
    assert_eq!(parsed.finish_reason, ProxyFinishReason::Stop);

    let re = serde_json::to_string(&parsed).expect("serialize");
    let back: ProxyCompletionResponse = serde_json::from_str(&re).expect("re-parse");
    assert_eq!(back.content, parsed.content);
    assert_eq!(back.input_tokens, parsed.input_tokens);
}

#[test]
fn job_description_from_fixture() {
    let fixture = serde_json::json!({
        "title": "Test Job",
        "description": "Do something",
        "project_dir": "/tmp/project"
    });
    let parsed: JobDescription = serde_json::from_value(fixture).expect("parse");
    insta::assert_json_snapshot!("job_description", &parsed);
    assert_eq!(parsed.title, "Test Job");
    assert_eq!(parsed.description, "Do something");
    assert_eq!(parsed.project_dir.as_deref(), Some("/tmp/project"));

    let re = serde_json::to_string(&parsed).expect("serialize");
    let back: JobDescription = serde_json::from_str(&re).expect("re-parse");
    assert_eq!(back.title, parsed.title);
    assert_eq!(back.description, parsed.description);
}

#[test]
fn remote_tool_catalog_response_from_fixture() {
    let fixture = serde_json::json!({
        "tools": [{"name": "t", "description": "d", "parameters": {"type": "object"}}],
        "toolset_instructions": ["Use bash carefully"],
        "catalog_version": 7
    });
    let parsed: RemoteToolCatalogResponse = serde_json::from_value(fixture).expect("parse");
    insta::assert_json_snapshot!("remote_tool_catalog_response", &parsed);
    assert_eq!(parsed.catalog_version, 7);

    let re = serde_json::to_string(&parsed).expect("serialize");
    let back: RemoteToolCatalogResponse = serde_json::from_str(&re).expect("re-parse");
    assert_eq!(back, parsed);
}

#[test]
fn credential_response_from_fixture() {
    let fixture = serde_json::json!({"env_var": "API_KEY", "value": "secret123"});
    let parsed: CredentialResponse = serde_json::from_value(fixture).expect("parse");
    insta::assert_json_snapshot!("credential_response", &parsed);
    assert_eq!(parsed.env_var, "API_KEY");
    assert_eq!(parsed.value, "secret123");
}

#[test]
fn prompt_response_from_fixture() {
    let fixture = serde_json::json!({"content": "Continue?", "done": false});
    let parsed: PromptResponse = serde_json::from_value(fixture).expect("parse");
    insta::assert_json_snapshot!("prompt_response", &parsed);
    assert_eq!(parsed.content, "Continue?");
    assert!(!parsed.done);
}

// ---------------------------------------------------------------------------
// ProxyFinishReason aliases
// ---------------------------------------------------------------------------

#[test]
fn finish_reason_tool_calls_alias() {
    let reason: ProxyFinishReason =
        serde_json::from_value(serde_json::json!("tool_calls")).expect("parse");
    assert_eq!(reason, ProxyFinishReason::ToolUse);
}

#[test]
fn finish_reason_unknown_fallback() {
    let reason: ProxyFinishReason =
        serde_json::from_value(serde_json::json!("some_novel_reason")).expect("parse");
    assert_eq!(reason, ProxyFinishReason::Unknown);
}
