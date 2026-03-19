//! Tests for the Claude Code bridge helpers.

use rstest::rstest;

use super::fs_setup::{build_permission_settings, copy_dir_recursive};
use super::ndjson::{
    ClaudeStreamEvent, ContentBlock, MessageWrapper, stream_event_to_payloads, truncate,
};
use crate::worker::api::{JobEventPayload, JobEventType};

macro_rules! stream_payload_case {
    ($fn_name:ident, $event:expr, [$($expected:expr),+ $(,)?]) => {
        fn $fn_name() -> StreamPayloadCase {
            StreamPayloadCase {
                event: $event,
                expected: vec![$($expected),+],
            }
        }
    };
}

#[test]
fn test_parse_system_event() {
    let json = r#"{"type":"system","session_id":"abc-123","subtype":"init"}"#;
    let event: ClaudeStreamEvent =
        serde_json::from_str(json).expect("failed to parse system event");
    assert_eq!(event.event_type, "system");
    assert_eq!(event.session_id.as_deref(), Some("abc-123"));
}

#[test]
fn test_parse_assistant_text_event() {
    let json = r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Hello world"}]}}"#;
    let event: ClaudeStreamEvent =
        serde_json::from_str(json).expect("failed to parse assistant text event");
    assert_eq!(event.event_type, "assistant");
    let blocks = event
        .message
        .expect("missing message")
        .content
        .expect("missing content");
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].block_type, "text");
    assert_eq!(blocks[0].text.as_deref(), Some("Hello world"));
}

#[test]
fn test_parse_assistant_tool_use_event() {
    let json = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_01abc","name":"Bash","input":{"command":"ls"}}]}}"#;
    let event: ClaudeStreamEvent =
        serde_json::from_str(json).expect("failed to parse assistant tool_use event");
    let blocks = event
        .message
        .expect("missing message")
        .content
        .expect("missing content");
    assert_eq!(blocks[0].block_type, "tool_use");
    assert_eq!(blocks[0].name.as_deref(), Some("Bash"));
    assert_eq!(blocks[0].id.as_deref(), Some("toolu_01abc"));
    assert!(blocks[0].input.is_some());
}

#[test]
fn test_parse_user_tool_result_event() {
    let json = r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01abc","content":"/workspace"}]}}"#;
    let event: ClaudeStreamEvent =
        serde_json::from_str(json).expect("failed to parse user tool_result event");
    assert_eq!(event.event_type, "user");
    let blocks = event
        .message
        .expect("missing message")
        .content
        .expect("missing content");
    assert_eq!(blocks[0].block_type, "tool_result");
    assert_eq!(blocks[0].tool_use_id.as_deref(), Some("toolu_01abc"));
}

#[test]
fn test_parse_result_event() {
    let json = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":5000,"num_turns":3,"result":"Done.","session_id":"sid-1"}"#;
    let event: ClaudeStreamEvent =
        serde_json::from_str(json).expect("failed to parse result event");
    assert_eq!(event.event_type, "result");
    assert_eq!(event.is_error, Some(false));
    assert_eq!(event.duration_ms, Some(5000));
    assert_eq!(event.num_turns, Some(3));
    assert_eq!(
        event
            .result
            .expect("missing result text")
            .as_str()
            .expect("result text should be a string"),
        "Done."
    );
}

#[test]
fn test_parse_result_error_event() {
    let json = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"duration_ms":60000,"num_turns":50}"#;
    let event: ClaudeStreamEvent =
        serde_json::from_str(json).expect("failed to parse error result event");
    assert_eq!(event.is_error, Some(true));
    assert_eq!(event.subtype.as_deref(), Some("error_max_turns"));
}

#[derive(Debug)]
struct ExpectedPayload {
    event_type: JobEventType,
    data: serde_json::Value,
}

#[derive(Debug)]
struct StreamPayloadCase {
    event: ClaudeStreamEvent,
    expected: Vec<ExpectedPayload>,
}

stream_payload_case!(
    make_stream_payload_case_system,
    ClaudeStreamEvent {
        event_type: "system".to_string(),
        session_id: Some("sid-123".to_string()),
        subtype: Some("init".to_string()),
        message: None,
        result: None,
        is_error: None,
        duration_ms: None,
        num_turns: None,
    },
    [ExpectedPayload {
        event_type: JobEventType::Status,
        data: serde_json::json!({
            "message": "Claude Code session started",
            "session_id": "sid-123",
        }),
    }]
);

stream_payload_case!(
    make_stream_payload_case_assistant_text,
    ClaudeStreamEvent {
        event_type: "assistant".to_string(),
        session_id: None,
        subtype: None,
        message: Some(MessageWrapper {
            role: Some("assistant".to_string()),
            content: Some(vec![ContentBlock {
                block_type: "text".to_string(),
                text: Some("Here's the answer".to_string()),
                name: None,
                id: None,
                input: None,
                content: None,
                tool_use_id: None,
            }]),
        }),
        result: None,
        is_error: None,
        duration_ms: None,
        num_turns: None,
    },
    [ExpectedPayload {
        event_type: JobEventType::Message,
        data: serde_json::json!({
            "role": "assistant",
            "content": "Here's the answer",
        }),
    }]
);

stream_payload_case!(
    make_stream_payload_case_assistant_tool_use,
    ClaudeStreamEvent {
        event_type: "assistant".to_string(),
        session_id: None,
        subtype: None,
        message: Some(MessageWrapper {
            role: Some("assistant".to_string()),
            content: Some(vec![ContentBlock {
                block_type: "tool_use".to_string(),
                text: None,
                name: Some("Bash".to_string()),
                id: Some("toolu_01abc".to_string()),
                input: Some(serde_json::json!({"command": "ls"})),
                content: None,
                tool_use_id: None,
            }]),
        }),
        result: None,
        is_error: None,
        duration_ms: None,
        num_turns: None,
    },
    [ExpectedPayload {
        event_type: JobEventType::ToolUse,
        data: serde_json::json!({
            "tool_name": "Bash",
            "tool_use_id": "toolu_01abc",
            "input": {"command": "ls"},
        }),
    }]
);

stream_payload_case!(
    make_stream_payload_case_user_tool_result,
    ClaudeStreamEvent {
        event_type: "user".to_string(),
        session_id: None,
        subtype: None,
        message: Some(MessageWrapper {
            role: Some("user".to_string()),
            content: Some(vec![ContentBlock {
                block_type: "tool_result".to_string(),
                text: None,
                name: None,
                id: None,
                input: None,
                content: Some(serde_json::json!("/workspace")),
                tool_use_id: Some("toolu_01abc".to_string()),
            }]),
        }),
        result: None,
        is_error: None,
        duration_ms: None,
        num_turns: None,
    },
    [ExpectedPayload {
        event_type: JobEventType::ToolResult,
        data: serde_json::json!({
            "tool_use_id": "toolu_01abc",
            "output": "/workspace",
        }),
    }]
);

stream_payload_case!(
    make_stream_payload_case_result_success,
    ClaudeStreamEvent {
        event_type: "result".to_string(),
        session_id: Some("s1".to_string()),
        subtype: Some("success".to_string()),
        message: None,
        result: Some(serde_json::json!("The review is complete.")),
        is_error: Some(false),
        duration_ms: Some(12000),
        num_turns: Some(5),
    },
    [
        ExpectedPayload {
            event_type: JobEventType::Message,
            data: serde_json::json!({
                "role": "assistant",
                "content": "The review is complete.",
            }),
        },
        ExpectedPayload {
            event_type: JobEventType::Result,
            data: serde_json::json!({
                "status": "completed",
                "session_id": "s1",
                "duration_ms": 12000,
                "num_turns": 5,
            }),
        }
    ]
);

stream_payload_case!(
    make_stream_payload_case_result_error,
    ClaudeStreamEvent {
        event_type: "result".to_string(),
        session_id: None,
        subtype: Some("error_max_turns".to_string()),
        message: None,
        result: None,
        is_error: Some(true),
        duration_ms: None,
        num_turns: None,
    },
    [ExpectedPayload {
        event_type: JobEventType::Result,
        data: serde_json::json!({
            "status": "error",
            "session_id": null,
            "duration_ms": null,
            "num_turns": null,
        }),
    }]
);

stream_payload_case!(
    make_stream_payload_case_unknown_type,
    ClaudeStreamEvent {
        event_type: "fancy_new_thing".to_string(),
        session_id: None,
        subtype: None,
        message: None,
        result: None,
        is_error: None,
        duration_ms: None,
        num_turns: None,
    },
    [ExpectedPayload {
        event_type: JobEventType::Status,
        data: serde_json::json!({
            "message": "Claude event: fancy_new_thing",
            "raw_type": "fancy_new_thing",
        }),
    }]
);

#[rstest]
#[case(make_stream_payload_case_system())]
#[case(make_stream_payload_case_assistant_text())]
#[case(make_stream_payload_case_assistant_tool_use())]
#[case(make_stream_payload_case_user_tool_result())]
#[case(make_stream_payload_case_result_success())]
#[case(make_stream_payload_case_result_error())]
#[case(make_stream_payload_case_unknown_type())]
fn test_stream_event_to_payloads(#[case] case: StreamPayloadCase) {
    let payloads = stream_event_to_payloads(&case.event);
    assert_eq!(payloads.len(), case.expected.len());

    for (payload, expected) in payloads.iter().zip(case.expected.iter()) {
        assert_eq!(payload.event_type, expected.event_type);
        assert_eq!(payload.data, expected.data);
    }
}

#[test]
fn test_claude_event_payload_serde() {
    let payload = JobEventPayload {
        event_type: JobEventType::Message,
        data: serde_json::json!({ "role": "assistant", "content": "hi" }),
    };
    let json = serde_json::to_string(&payload).expect("failed to serialize JobEventPayload");
    let parsed: JobEventPayload =
        serde_json::from_str(&json).expect("failed to deserialize JobEventPayload");
    assert_eq!(parsed.event_type, JobEventType::Message);
    assert_eq!(parsed.data["content"], "hi");
}

#[test]
fn test_truncate() {
    assert_eq!(truncate("hello", 10), "hello");
    assert_eq!(truncate("hello world", 5), "hello");
    assert_eq!(truncate("", 5), "");
}

#[test]
fn test_build_permission_settings_default_tools() {
    let tools: Vec<String> = ["Bash(*)", "Read", "Edit(*)", "Glob", "Grep"]
        .into_iter()
        .map(String::from)
        .collect();
    let json_str =
        build_permission_settings(&tools).expect("default tool permission settings should build");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("settings JSON should parse");
    let allow = parsed["permissions"]["allow"]
        .as_array()
        .expect("allow list should be an array");
    assert_eq!(allow.len(), 5);
    assert_eq!(allow[0], "Bash(*)");
    assert_eq!(allow[1], "Read");
    assert_eq!(allow[2], "Edit(*)");
}

#[test]
fn test_build_permission_settings_empty_tools() {
    let json_str =
        build_permission_settings(&[]).expect("empty tool permission settings should build");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("settings JSON should parse");
    let allow = parsed["permissions"]["allow"]
        .as_array()
        .expect("allow list should be an array");
    assert!(allow.is_empty());
}

#[test]
fn test_build_permission_settings_is_valid_json() {
    let tools = vec!["Bash(npm run *)".to_string(), "Read".to_string()];
    let json_str =
        build_permission_settings(&tools).expect("permission settings JSON should build");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_str).expect("settings JSON should parse");
    assert!(parsed["permissions"].is_object());
    assert!(parsed["permissions"]["allow"].is_array());
}

#[test]
fn test_copy_dir_recursive() {
    let src = tempfile::tempdir().expect("create src tempdir");
    let dst = tempfile::tempdir().expect("create dst tempdir");

    std::fs::write(src.path().join("auth.json"), r#"{"token":"abc"}"#).expect("write auth file");
    std::fs::create_dir_all(src.path().join("subdir")).expect("create subdir");
    std::fs::write(src.path().join("subdir").join("nested.txt"), "nested")
        .expect("write nested file");

    let copied = copy_dir_recursive(src.path(), dst.path()).expect("copy directory tree");
    assert_eq!(copied, 2);
    assert_eq!(
        std::fs::read_to_string(dst.path().join("auth.json")).expect("read copied auth file"),
        r#"{"token":"abc"}"#
    );
    assert_eq!(
        std::fs::read_to_string(dst.path().join("subdir").join("nested.txt"))
            .expect("read copied nested file"),
        "nested"
    );
}

#[test]
fn test_copy_dir_recursive_empty_source() {
    let src = tempfile::tempdir().expect("create src tempdir");
    let dst = tempfile::tempdir().expect("create dst tempdir");

    let copied = copy_dir_recursive(src.path(), dst.path()).expect("copy empty directory");
    assert_eq!(copied, 0);
}

#[test]
fn test_copy_dir_recursive_skips_nonexistent_source() {
    let dst = tempfile::tempdir().expect("create dst tempdir");
    let nonexistent = std::path::Path::new("/no/such/path");

    let copied = copy_dir_recursive(nonexistent, dst.path()).expect("copy should be graceful");
    assert_eq!(copied, 0);
}
