//! Tests for destructive-command approval: explicit approval patterns and
//! command extraction from tool-call arguments.

use crate::tools::tool::{ApprovalRequirement, NativeTool};

use super::super::{ShellTool, requires_explicit_approval};

#[test]
fn test_requires_explicit_approval() {
    // Destructive commands should require explicit approval
    assert!(requires_explicit_approval("rm -rf /tmp/stuff"));
    assert!(requires_explicit_approval("git push --force origin main"));
    assert!(requires_explicit_approval("git reset --hard HEAD~5"));
    assert!(requires_explicit_approval("docker rm container_name"));
    assert!(requires_explicit_approval("kill -9 12345"));
    assert!(requires_explicit_approval("DROP TABLE users;"));

    // Safe commands should not
    assert!(!requires_explicit_approval("cargo build"));
    assert!(!requires_explicit_approval("git status"));
    assert!(!requires_explicit_approval("ls -la"));
    assert!(!requires_explicit_approval("echo hello"));
    assert!(!requires_explicit_approval("cat file.txt"));
    assert!(!requires_explicit_approval(
        "git push origin feature-branch"
    ));
}

/// Replicate the extraction logic from agent_loop.rs to prove it works
/// when `arguments` is a `serde_json::Value::Object` (the common case
/// that was previously broken because `Value::Object.as_str()` returns None).
#[test]
fn test_destructive_command_extraction_from_object_args() {
    let arguments = serde_json::json!({"command": "rm -rf /tmp/stuff"});

    let cmd = arguments
        .get("command")
        .and_then(|c| c.as_str().map(String::from))
        .or_else(|| {
            arguments
                .as_str()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .and_then(|v| v.get("command").and_then(|c| c.as_str().map(String::from)))
        });

    assert_eq!(cmd.as_deref(), Some("rm -rf /tmp/stuff"));
    assert!(requires_explicit_approval(cmd.as_deref().unwrap()));
}

/// Verify extraction still works when `arguments` is a JSON string
/// (rare, but possible if the LLM provider returns string-encoded JSON).
#[test]
fn test_destructive_command_extraction_from_string_args() {
    let arguments =
        serde_json::Value::String(r#"{"command": "git push --force origin main"}"#.to_string());

    let cmd = arguments
        .get("command")
        .and_then(|c| c.as_str().map(String::from))
        .or_else(|| {
            arguments
                .as_str()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .and_then(|v| v.get("command").and_then(|c| c.as_str().map(String::from)))
        });

    assert_eq!(cmd.as_deref(), Some("git push --force origin main"));
    assert!(requires_explicit_approval(cmd.as_deref().unwrap()));
}

#[test]
fn test_requires_approval_destructive_command() {
    let tool = ShellTool::new();
    // Destructive commands must return Always to bypass auto-approve.
    assert_eq!(
        tool.requires_approval(&serde_json::json!({"command": "rm -rf /tmp"})),
        ApprovalRequirement::Always
    );
    assert_eq!(
        tool.requires_approval(&serde_json::json!({"command": "git push --force origin main"})),
        ApprovalRequirement::Always
    );
    assert_eq!(
        tool.requires_approval(&serde_json::json!({"command": "DROP TABLE users;"})),
        ApprovalRequirement::Always
    );
}

#[test]
fn test_requires_approval_safe_command() {
    let tool = ShellTool::new();
    // Safe commands return UnlessAutoApproved (can be auto-approved).
    assert_eq!(
        tool.requires_approval(&serde_json::json!({"command": "cargo build"})),
        ApprovalRequirement::UnlessAutoApproved
    );
    assert_eq!(
        tool.requires_approval(&serde_json::json!({"command": "echo hello"})),
        ApprovalRequirement::UnlessAutoApproved
    );
}

#[test]
fn test_requires_approval_string_encoded_args() {
    let tool = ShellTool::new();
    // When arguments are string-encoded JSON (rare LLM behavior).
    let args = serde_json::Value::String(r#"{"command": "rm -rf /tmp/stuff"}"#.to_string());
    assert_eq!(tool.requires_approval(&args), ApprovalRequirement::Always);
}

#[test]
fn test_approval_with_mixed_case_destructive() {
    // Case-insensitive destructive command detection
    assert!(requires_explicit_approval("RM -RF /tmp"));
    assert!(requires_explicit_approval("Git Push --Force origin main"));
    assert!(requires_explicit_approval("DROP table users;"));
}
