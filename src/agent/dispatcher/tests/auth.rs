//! Auth detection tests.

use super::super::{check_auth_required, parse_auth_result};
use super::*;

/// Serialise `json` as a successful `Result<String, Error>` and call
/// `check_auth_required`. Eliminates the repeated two-line setup in every
/// auth detection test.
fn check_auth_json(tool_name: &str, json: serde_json::Value) -> Option<(String, String)> {
    let result: Result<String, Error> = Ok(json.to_string());
    check_auth_required(tool_name, &result)
}

/// Assert that an auth-awaiting detection result is `Some`, and that the
/// returned name and instructions match the expected values.
fn assert_auth_detected(
    detected: Option<(String, String)>,
    expected_name: &str,
    expected_instructions_fragment: &str,
) {
    assert!(detected.is_some(), "expected auth detection to fire");
    let (name, instructions) =
        detected.expect("expected auth detection to fire and return (name, instructions)");
    assert_eq!(name, expected_name);
    assert!(
        instructions.contains(expected_instructions_fragment),
        "instructions did not contain {:?}: {:?}",
        expected_instructions_fragment,
        instructions,
    );
}

#[test]
fn test_detect_auth_awaiting_positive() {
    assert_auth_detected(
        check_auth_json(
            "tool_auth",
            serde_json::json!({
                "name": "telegram",
                "kind": "WasmTool",
                "awaiting_token": true,
                "status": "awaiting_token",
                "instructions": "Please provide your Telegram Bot API token."
            }),
        ),
        "telegram",
        "Telegram Bot API",
    );
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

    let (_, instructions) = check_auth_required("tool_auth", &result)
        .expect("expected auth detection to fire for tool_auth with awaiting_token");
    assert_eq!(instructions, "Please provide your API token/key.");
}

#[test]
fn test_detect_auth_awaiting_type_field_without_name() {
    let result: Result<String, Error> = Ok(serde_json::json!({
        "type": "awaiting_token",
        "instructions": "Visit the auth flow."
    })
    .to_string());

    let (name, instructions) = check_auth_required("tool_auth", &result)
        .expect("expected auth detection to fire for type=awaiting_token");

    assert_eq!(name, "tool_auth");
    assert_eq!(instructions, "Visit the auth flow.");
}

#[test]
fn test_detect_auth_awaiting_tool_activate() {
    assert_auth_detected(
        check_auth_json(
            "tool_activate",
            serde_json::json!({
                "name": "slack",
                "kind": "McpServer",
                "awaiting_token": true,
                "status": "awaiting_token",
                "instructions": "Provide your Slack Bot token."
            }),
        ),
        "slack",
        "Slack Bot",
    );
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

// Tests for parse_auth_result

#[rstest::rstest]
#[case::auth_url_only(
    Ok(serde_json::json!({ "auth_url": "https://example.com/auth" }).to_string()),
    Some("https://example.com/auth"),
    None,
)]
#[case::setup_url_only(
    Ok(serde_json::json!({ "setup_url": "https://example.com/setup" }).to_string()),
    None,
    Some("https://example.com/setup"),
)]
#[case::neither_url(
    Ok(serde_json::json!({ "message": "no urls here" }).to_string()),
    None,
    None,
)]
#[case::malformed_json(
    Ok("this is not json".to_string()),
    None,
    None,
)]
#[case::error_result(
    Err::<String, Error>(crate::error::ToolError::NotFound { name: "x".into() }.into()),
    None,
    None,
)]
fn test_parse_auth_result(
    #[case] result: Result<String, Error>,
    #[case] expected_auth_url: Option<&str>,
    #[case] expected_setup_url: Option<&str>,
) {
    let parsed = parse_auth_result(&result);

    assert_eq!(parsed.auth_url.as_deref(), expected_auth_url);
    assert_eq!(parsed.setup_url.as_deref(), expected_setup_url);
}
