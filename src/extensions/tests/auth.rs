//! Unit tests for extension authentication results: constructors,
//! accessors, wire-format serde round trips, and status strings.

use crate::extensions::{AuthResult, AuthStatus, ExtensionKind, ToolAuthState};

#[test]
fn auth_result_authenticated_round_trip() {
    let result = AuthResult::authenticated("gmail", ExtensionKind::WasmTool);
    let json = serde_json::to_value(&result).unwrap();

    assert_eq!(json["status"], "authenticated");
    assert_eq!(json["name"], "gmail");
    assert_eq!(json["kind"], "wasm_tool");
    assert_eq!(json["awaiting_token"], false);
    assert!(json.get("auth_url").is_none());
    assert!(json.get("instructions").is_none());

    let back: AuthResult = serde_json::from_value(json).unwrap();
    assert!(back.is_authenticated());
    assert!(back.auth_url().is_none());
}

#[test]
fn auth_result_awaiting_authorization_round_trip() {
    let result = AuthResult::awaiting_authorization(
        "google-drive",
        ExtensionKind::WasmTool,
        "https://accounts.google.com/o/oauth2/v2/auth?state=abc".to_string(),
        "local".to_string(),
    );
    let json = serde_json::to_value(&result).unwrap();

    assert_eq!(json["status"], "awaiting_authorization");
    assert_eq!(
        json["auth_url"],
        "https://accounts.google.com/o/oauth2/v2/auth?state=abc"
    );
    assert_eq!(json["callback_type"], "local");
    assert_eq!(json["awaiting_token"], false);

    let back: AuthResult = serde_json::from_value(json).unwrap();
    assert_eq!(
        back.auth_url(),
        Some("https://accounts.google.com/o/oauth2/v2/auth?state=abc")
    );
    assert_eq!(back.callback_type(), Some("local"));
    assert!(!back.is_authenticated());
}

#[test]
fn auth_result_awaiting_token_round_trip() {
    let result = AuthResult::awaiting_token(
        "telegram",
        ExtensionKind::WasmChannel,
        "Enter your bot token".to_string(),
        None,
    );
    let json = serde_json::to_value(&result).unwrap();

    assert_eq!(json["status"], "awaiting_token");
    assert_eq!(json["instructions"], "Enter your bot token");
    assert_eq!(json["awaiting_token"], true);
    assert!(json.get("auth_url").is_none());

    let back: AuthResult = serde_json::from_value(json).unwrap();
    assert!(back.is_awaiting_token());
    assert_eq!(back.instructions(), Some("Enter your bot token"));
}

#[test]
fn auth_result_needs_setup_round_trip() {
    let result = AuthResult::needs_setup(
        "custom-tool",
        ExtensionKind::WasmTool,
        "Configure OAuth credentials in the Setup tab.".to_string(),
        Some("https://console.cloud.google.com".to_string()),
    );
    let json = serde_json::to_value(&result).unwrap();

    assert_eq!(json["status"], "needs_setup");
    assert_eq!(json["setup_url"], "https://console.cloud.google.com");
    assert_eq!(json["awaiting_token"], false);

    let back: AuthResult = serde_json::from_value(json).unwrap();
    assert!(!back.is_authenticated());
    assert!(!back.is_awaiting_token());
    assert_eq!(back.setup_url(), Some("https://console.cloud.google.com"));
}

#[test]
fn auth_result_no_auth_required_round_trip() {
    let result = AuthResult::no_auth_required("echo", ExtensionKind::WasmTool);
    let json = serde_json::to_value(&result).unwrap();

    assert_eq!(json["status"], "no_auth_required");
    assert_eq!(json["awaiting_token"], false);

    let back: AuthResult = serde_json::from_value(json).unwrap();
    assert!(!back.is_authenticated());
    assert_eq!(back.status, AuthStatus::NoAuthRequired);
}

#[test]
fn auth_status_type_safety() {
    // AwaitingAuthorization always has auth_url
    let result = AuthResult::awaiting_authorization(
        "test",
        ExtensionKind::WasmTool,
        "https://example.com".to_string(),
        "local".to_string(),
    );
    assert!(result.auth_url().is_some());
    assert!(!result.is_awaiting_token());

    // Authenticated never has auth_url
    let result = AuthResult::authenticated("test", ExtensionKind::WasmTool);
    assert!(result.auth_url().is_none());
    assert!(result.instructions().is_none());
    assert!(result.setup_url().is_none());
}

// ── ToolAuthState ────────────────────────────────────────────────

#[test]
fn tool_auth_state_equality() {
    assert_eq!(ToolAuthState::Ready, ToolAuthState::Ready);
    assert_eq!(ToolAuthState::NeedsAuth, ToolAuthState::NeedsAuth);
    assert_eq!(ToolAuthState::NeedsSetup, ToolAuthState::NeedsSetup);
    assert_eq!(ToolAuthState::NoAuth, ToolAuthState::NoAuth);

    assert_ne!(ToolAuthState::Ready, ToolAuthState::NeedsAuth);
    assert_ne!(ToolAuthState::NeedsSetup, ToolAuthState::NoAuth);
    assert_ne!(ToolAuthState::Ready, ToolAuthState::NoAuth);
}

// ── AuthResult::status_str ───────────────────────────────────────

#[test]
fn auth_result_status_str_all_variants() {
    assert_eq!(
        AuthResult::authenticated("a", ExtensionKind::McpServer).status_str(),
        "authenticated"
    );
    assert_eq!(
        AuthResult::no_auth_required("b", ExtensionKind::WasmTool).status_str(),
        "no_auth_required"
    );
    assert_eq!(
        AuthResult::awaiting_authorization(
            "c",
            ExtensionKind::WasmChannel,
            "https://x.com".into(),
            "local".into(),
        )
        .status_str(),
        "awaiting_authorization"
    );
    assert_eq!(
        AuthResult::awaiting_token("d", ExtensionKind::WasmTool, "paste token".into(), None)
            .status_str(),
        "awaiting_token"
    );
    assert_eq!(
        AuthResult::needs_setup(
            "e",
            ExtensionKind::McpServer,
            "configure oauth".into(),
            Some("https://setup.example.com".into()),
        )
        .status_str(),
        "needs_setup"
    );
}
