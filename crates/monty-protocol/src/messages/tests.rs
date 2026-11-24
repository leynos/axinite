//! Unit tests for the parent/helper message contracts.

use super::*;

fn limits() -> GuestLimits {
    GuestLimits {
        max_memory_bytes: 67_108_864,
        max_recursion_depth: 256,
        max_suspensions: 4096,
        max_feed_millis: 30_000,
        max_turn_millis: 30_000,
        max_output_bytes: 65_536,
    }
}

fn start(tools: Vec<ToolDefinition>) -> StartRun {
    StartRun {
        run_id: "run_1".to_owned(),
        code: "return {'result': params, 'state': state}".to_owned(),
        entrypoint: "main".to_owned(),
        params: serde_json::json!({}),
        state: serde_json::json!({}),
        tools,
        limits: limits(),
    }
}

fn tool(name: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_owned(),
        description: String::new(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    }
}

#[test]
fn accepts_a_valid_start() {
    validate_start_run(&start(vec![tool("echo")])).expect("valid");
}

#[test]
fn rejects_reserved_entrypoint() {
    let mut request = start(Vec::new());
    request.entrypoint = "state".to_owned();
    assert!(validate_start_run(&request).is_err());
}

#[test]
fn rejects_duplicate_tools() {
    let request = start(vec![tool("echo"), tool("echo")]);
    assert!(validate_start_run(&request).is_err());
}

#[test]
fn rejects_tool_that_shadows_entrypoint() {
    let request = start(vec![tool("main")]);
    assert!(validate_start_run(&request).is_err());
}

#[test]
fn rejects_invalid_run_id() {
    let mut request = start(Vec::new());
    request.run_id = "bad id".to_owned();
    assert!(validate_start_run(&request).is_err());
}

#[test]
fn rejects_help_to_parent_unknown_field() {
    let payload = serde_json::json!({
        "kind": "cancel_run",
        "run_id": "r",
        "extra": 1
    });
    let error = serde_json::from_value::<ParentToHelper>(payload).expect_err("unknown");
    assert!(error.to_string().contains("unknown field"), "{error}");
}

#[test]
fn rejects_terminal_unknown_field() {
    let payload = serde_json::json!({
        "kind": "terminal",
        "run_id": "r",
        "outcome": {"status": "script_error", "message": "x"},
        "extra": 1
    });
    let error = serde_json::from_value::<HelperToParent>(payload).expect_err("unknown");
    assert!(error.to_string().contains("unknown field"), "{error}");
}
