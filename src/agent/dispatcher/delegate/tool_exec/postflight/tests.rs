//! Unit tests for parsing tool authentication barrier payloads.

use super::*;

#[test]
fn parse_auth_barrier_returns_urls_when_present() {
    let result = Ok(
        r#"{"awaiting_token":true,"name":"ngrok","instructions":"visit https://example.com","auth_url":"https://example.com/auth","setup_url":"https://example.com/setup"}"#
            .to_string(),
    );

    let parsed =
        parse_auth_barrier("tool_auth", &result).expect("auth barrier payload should parse");

    assert_eq!(
        parsed.auth_url,
        Some("https://example.com/auth".to_string())
    );
    assert_eq!(
        parsed.setup_url,
        Some("https://example.com/setup".to_string())
    );
}

#[test]
fn parse_auth_barrier_returns_none_for_err_result() {
    let result = Err(crate::error::ToolError::ExecutionFailed {
        name: "tool_auth".to_string(),
        reason: "boom".to_string(),
    }
    .into());

    assert!(parse_auth_barrier("tool_auth", &result).is_none());
}

#[test]
fn check_auth_required_returns_none_for_plain_output() {
    let result = Ok("plain output".to_string());

    assert!(check_auth_required("tool_auth", &result).is_none());
}

#[test]
fn check_auth_required_returns_some_for_awaiting_token() {
    let payload = r#"{"awaiting_token":true,"name":"ngrok","instructions":"visit https://x.com"}"#;
    let result = Ok(payload.to_string());

    let (extension_name, instructions) = check_auth_required("tool_auth", &result)
        .expect("awaiting token payload should require auth");

    assert_eq!(extension_name, "ngrok");
    assert!(instructions.contains("visit"));
}
