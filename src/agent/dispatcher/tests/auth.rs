//! Auth detection tests.

use super::super::check_auth_required;
use super::*;

/// Serialise `json` as a successful `Result<String, Error>` and call
/// `check_auth_required`. Eliminates the repeated two-line setup in every
/// auth detection test.
fn check_auth_json(tool_name: &str, json: serde_json::Value) -> Option<(String, String)> {
    let result: Result<String, Error> = Ok(json.to_string());
    check_auth_required(tool_name, &result)
}

#[test]
fn test_detect_auth_awaiting_positive() {
    let detected = check_auth_json(
        "tool_auth",
        serde_json::json!({
            "name": "telegram",
            "kind": "WasmTool",
            "awaiting_token": true,
            "status": "awaiting_token",
            "instructions": "Please provide your Telegram Bot API token."
        }),
    );
    assert!(detected.is_some());
    let (name, instructions) = detected.unwrap();
    assert_eq!(name, "telegram");
    assert!(instructions.contains("Telegram Bot API"));
}

#[test]
fn test_detect_auth_awaiting_not_awaiting() {
    assert!(
        check_auth_json(
            "tool_auth",
            serde_json::json!({
                "name": "telegram",
                "kind": "WasmTool",
                "awaiting_token": false,
                "status": "authenticated"
            })
        )
        .is_none()
    );
}

#[test]
fn test_detect_auth_awaiting_wrong_tool() {
    assert!(
        check_auth_json(
            "tool_list",
            serde_json::json!({
                "name": "telegram",
                "awaiting_token": true,
            })
        )
        .is_none()
    );
}

#[test]
fn test_detect_auth_awaiting_error_result() {
    let result: Result<String, Error> =
        Err(crate::error::ToolError::NotFound { name: "x".into() }.into());
    assert!(check_auth_required("tool_auth", &result).is_none());
}

#[test]
fn test_detect_auth_awaiting_default_instructions() {
    let result: Result<String, Error> = Ok(serde_json::json!({
        "name": "custom_tool",
        "awaiting_token": true,
        "status": "awaiting_token"
    })
    .to_string());

    let (_, instructions) = check_auth_required("tool_auth", &result).unwrap();
    assert_eq!(instructions, "Please provide your API token/key.");
}

#[test]
fn test_detect_auth_awaiting_tool_activate() {
    let detected = check_auth_json(
        "tool_activate",
        serde_json::json!({
            "name": "slack",
            "kind": "McpServer",
            "awaiting_token": true,
            "status": "awaiting_token",
            "instructions": "Provide your Slack Bot token."
        }),
    );
    assert!(detected.is_some());
    let (name, instructions) = detected.unwrap();
    assert_eq!(name, "slack");
    assert!(instructions.contains("Slack Bot"));
}

#[test]
fn test_detect_auth_awaiting_tool_activate_not_awaiting() {
    let result: Result<String, Error> = Ok(serde_json::json!({
        "name": "slack",
        "tools_loaded": ["slack_post_message"],
        "message": "Activated"
    })
    .to_string());

    assert!(check_auth_required("tool_activate", &result).is_none());
}
