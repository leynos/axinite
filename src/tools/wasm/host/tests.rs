//! Unit tests for WASM host state, logging limits, and path validation.

use std::sync::Arc;

use crate::tools::wasm::capabilities::{
    Capabilities, SecretsCapability, WorkspaceCapability, WorkspaceReader,
};
use crate::tools::wasm::host::{
    HostState, LogLevel, MAX_LOG_ENTRIES, MAX_LOG_MESSAGE_BYTES, validate_workspace_path,
};

struct MockReader {
    content: String,
}

impl WorkspaceReader for MockReader {
    fn read(&self, _path: &str) -> Option<String> {
        Some(self.content.clone())
    }
}

#[test]
fn test_logging_basic() {
    let mut state = HostState::minimal();
    state
        .log(LogLevel::Info, "test message".to_string())
        .unwrap();

    let logs = state.take_logs();
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].level, LogLevel::Info);
    assert_eq!(logs[0].message, "test message");
}

#[test]
fn test_logging_rate_limit() {
    let mut state = HostState::minimal();

    // Fill up to limit
    for i in 0..MAX_LOG_ENTRIES {
        state
            .log(LogLevel::Debug, format!("message {}", i))
            .unwrap();
    }

    // This should be dropped silently
    state
        .log(LogLevel::Info, "should be dropped".to_string())
        .unwrap();

    assert_eq!(state.take_logs().len(), MAX_LOG_ENTRIES);
    assert_eq!(state.logs_dropped(), 1);
}

#[test]
fn test_logging_truncation() {
    let mut state = HostState::minimal();

    let long_message = "x".repeat(MAX_LOG_MESSAGE_BYTES + 1000);
    state.log(LogLevel::Info, long_message).unwrap();

    let logs = state.take_logs();
    assert!(logs[0].message.len() <= MAX_LOG_MESSAGE_BYTES + 20); // +20 for truncation suffix
    assert!(logs[0].message.ends_with("... (truncated)"));
}

#[test]
fn test_now_millis() {
    let state = HostState::minimal();
    let now = state.now_millis();
    // Should be a reasonable timestamp (after 2020)
    assert!(now > 1577836800000); // Jan 1, 2020
}

#[test]
fn test_workspace_read_no_capability() {
    let state = HostState::minimal();
    let result = state.workspace_read("context/test.md").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_workspace_read_with_capability() {
    let reader = Arc::new(MockReader {
        content: "test content".to_string(),
    });

    let capabilities = Capabilities {
        workspace_read: Some(WorkspaceCapability {
            allowed_prefixes: vec![],
            reader: Some(reader),
        }),
        ..Default::default()
    };

    let state = HostState::new(capabilities);
    let result = state.workspace_read("context/test.md").unwrap();
    assert_eq!(result, Some("test content".to_string()));
}

#[test]
fn test_workspace_read_prefix_restriction() {
    let reader = Arc::new(MockReader {
        content: "test content".to_string(),
    });

    let capabilities = Capabilities {
        workspace_read: Some(WorkspaceCapability {
            allowed_prefixes: vec!["context/".to_string()],
            reader: Some(reader),
        }),
        ..Default::default()
    };

    let state = HostState::new(capabilities);

    // Allowed prefix
    let result = state.workspace_read("context/test.md").unwrap();
    assert!(result.is_some());

    // Disallowed prefix
    let result = state.workspace_read("secrets/api_key.txt").unwrap();
    assert!(result.is_none());
}

#[test]
fn test_path_validation_blocks_traversal() {
    assert!(validate_workspace_path("../etc/passwd").is_err());
    assert!(validate_workspace_path("context/../secrets").is_err());
    assert!(validate_workspace_path("context/test/../../secrets").is_err());
}

#[test]
fn test_path_validation_blocks_absolute() {
    assert!(validate_workspace_path("/etc/passwd").is_err());
    assert!(validate_workspace_path("/context/test.md").is_err());
}

#[test]
fn test_path_validation_blocks_null_bytes() {
    assert!(validate_workspace_path("context/test\0.md").is_err());
}

#[test]
fn test_path_validation_blocks_windows_paths() {
    assert!(validate_workspace_path("C:\\Windows\\System32").is_err());
    assert!(validate_workspace_path("D:secrets").is_err());
}

#[test]
fn test_path_validation_allows_valid_paths() {
    assert!(validate_workspace_path("context/test.md").is_ok());
    assert!(validate_workspace_path("daily/2024-01-15.md").is_ok());
    assert!(validate_workspace_path("projects/alpha/notes.md").is_ok());
    assert!(validate_workspace_path("MEMORY.md").is_ok());
}

#[test]
fn test_secret_exists_no_capability() {
    let state = HostState::minimal();
    assert!(!state.secret_exists("any_secret"));
}

#[test]
fn test_secret_exists_with_capability() {
    let capabilities = Capabilities {
        secrets: Some(SecretsCapability {
            allowed_names: vec!["openai_*".to_string(), "exact_name".to_string()],
        }),
        ..Default::default()
    };

    let state = HostState::new(capabilities);

    // Glob match
    assert!(state.secret_exists("openai_key"));
    assert!(state.secret_exists("openai_org"));

    // Exact match
    assert!(state.secret_exists("exact_name"));

    // Not allowed
    assert!(!state.secret_exists("stripe_key"));
}

#[test]
fn test_http_request_rate_limit() {
    // Create state with HTTP capability enabled
    let capabilities = Capabilities {
        http: Some(crate::tools::wasm::capabilities::HttpCapability::default()),
        ..Default::default()
    };
    let mut state = HostState::new(capabilities);

    // Should allow up to 50 requests
    for _ in 0..50 {
        assert!(state.record_http_request().is_ok());
    }

    // 51st should fail
    assert!(state.record_http_request().is_err());
}

#[test]
fn test_tool_invoke_rate_limit() {
    // Create state with tool invoke capability enabled
    let capabilities = Capabilities {
        tool_invoke: Some(crate::tools::wasm::capabilities::ToolInvokeCapability::default()),
        ..Default::default()
    };
    let mut state = HostState::new(capabilities);

    // Should allow up to 20 invocations
    for _ in 0..20 {
        assert!(state.record_tool_invoke().is_ok());
    }

    // 21st should fail
    assert!(state.record_tool_invoke().is_err());
}

#[test]
fn test_new_with_user() {
    let state = HostState::new_with_user(Capabilities::default(), "user123");
    assert_eq!(state.user_id(), Some("user123"));
}
