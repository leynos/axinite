//! Tests for NDJSON deserialization of Claude stream events.

use super::ClaudeStreamEvent;

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
