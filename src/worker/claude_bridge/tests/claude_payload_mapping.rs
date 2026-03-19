use rstest::rstest;

use super::{
    ClaudeStreamEvent, ContentBlock, JobEventPayload, JobEventType, MessageWrapper,
    stream_event_to_payloads,
};

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
