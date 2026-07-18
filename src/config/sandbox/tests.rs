//! Unit tests for sandbox configuration defaults and overrides.

use crate::config::sandbox::*;
use crate::testing::credentials::*;

// ── SandboxModeConfig defaults ──────────────────────────────────

#[test]
fn sandbox_mode_config_default_values() {
    let cfg = SandboxModeConfig::default();
    assert!(cfg.enabled);
    assert_eq!(cfg.policy, "readonly");
    assert_eq!(cfg.timeout_secs, 120);
    assert_eq!(cfg.memory_limit_mb, 2048);
    assert_eq!(cfg.cpu_shares, 1024);
    assert_eq!(cfg.image, "axinite-worker:latest");
    assert!(cfg.auto_pull_image);
    assert!(cfg.extra_allowed_domains.is_empty());
}

#[test]
fn sandbox_mode_config_custom_values() {
    let cfg = SandboxModeConfig {
        enabled: false,
        policy: "full_access".to_string(),
        timeout_secs: 600,
        memory_limit_mb: 4096,
        cpu_shares: 512,
        image: "custom-worker:v2".to_string(),
        auto_pull_image: false,
        extra_allowed_domains: vec!["example.com".to_string()],
        reaper_interval_secs: 300,
        orphan_threshold_secs: 600,
    };
    assert!(!cfg.enabled);
    assert_eq!(cfg.policy, "full_access");
    assert_eq!(cfg.timeout_secs, 600);
    assert_eq!(cfg.memory_limit_mb, 4096);
    assert_eq!(cfg.cpu_shares, 512);
    assert_eq!(cfg.image, "custom-worker:v2");
    assert!(!cfg.auto_pull_image);
    assert_eq!(cfg.extra_allowed_domains, vec!["example.com"]);
}

#[test]
fn sandbox_mode_to_sandbox_config_propagates_fields() {
    let mode = SandboxModeConfig {
        enabled: true,
        policy: "workspace_write".to_string(),
        timeout_secs: 300,
        memory_limit_mb: 1024,
        cpu_shares: 2048,
        image: "test:latest".to_string(),
        auto_pull_image: false,
        extra_allowed_domains: vec!["custom.example.com".to_string()],
        reaper_interval_secs: 300,
        orphan_threshold_secs: 600,
    };
    let sc = mode.to_sandbox_config();
    assert!(sc.enabled);
    assert_eq!(sc.policy, crate::sandbox::SandboxPolicy::WorkspaceWrite);
    assert_eq!(sc.timeout, std::time::Duration::from_secs(300));
    assert_eq!(sc.memory_limit_mb, 1024);
    assert_eq!(sc.cpu_shares, 2048);
    assert_eq!(sc.image, "test:latest");
    assert!(!sc.auto_pull_image);
    // extra domain should be in the allowlist
    assert!(
        sc.network_allowlist
            .contains(&"custom.example.com".to_string()),
        "expected custom domain in allowlist"
    );
}

#[test]
fn sandbox_mode_to_sandbox_config_invalid_policy_falls_back_to_readonly() {
    let mode = SandboxModeConfig {
        policy: "garbage_value".to_string(),
        ..SandboxModeConfig::default()
    };
    let sc = mode.to_sandbox_config();
    assert_eq!(sc.policy, crate::sandbox::SandboxPolicy::ReadOnly);
}

#[test]
fn sandbox_mode_to_sandbox_config_includes_default_allowlist() {
    let mode = SandboxModeConfig::default();
    let sc = mode.to_sandbox_config();
    // The default allowlist from sandbox module should be non-empty
    assert!(
        !sc.network_allowlist.is_empty(),
        "default allowlist should not be empty"
    );
}

// ── ClaudeCodeConfig defaults ───────────────────────────────────

#[test]
fn claude_code_config_default_values() {
    let cfg = ClaudeCodeConfig::default();
    assert!(!cfg.enabled);
    assert_eq!(cfg.model, "sonnet");
    assert_eq!(cfg.max_turns, 50);
    assert_eq!(cfg.memory_limit_mb, 4096);
    assert!(cfg.config_dir.ends_with(".claude"));
    // Should have all the standard tools
    assert!(!cfg.allowed_tools.is_empty());
    assert!(cfg.allowed_tools.contains(&"Bash(*)".to_string()));
    assert!(cfg.allowed_tools.contains(&"Read(*)".to_string()));
    assert!(cfg.allowed_tools.contains(&"Edit(*)".to_string()));
    assert!(cfg.allowed_tools.contains(&"Write(*)".to_string()));
    assert!(cfg.allowed_tools.contains(&"Grep(*)".to_string()));
    assert!(cfg.allowed_tools.contains(&"WebFetch(*)".to_string()));
}

#[test]
fn claude_code_config_custom_values() {
    let cfg = ClaudeCodeConfig {
        enabled: true,
        config_dir: std::path::PathBuf::from("/opt/claude"),
        model: "opus".to_string(),
        max_turns: 100,
        memory_limit_mb: 8192,
        allowed_tools: vec!["Read(*)".to_string(), "Bash(*)".to_string()],
    };
    assert!(cfg.enabled);
    assert_eq!(cfg.config_dir, std::path::PathBuf::from("/opt/claude"));
    assert_eq!(cfg.model, "opus");
    assert_eq!(cfg.max_turns, 100);
    assert_eq!(cfg.memory_limit_mb, 8192);
    assert_eq!(cfg.allowed_tools.len(), 2);
}

// ── parse_oauth_access_token ────────────────────────────────────

#[test]
fn parse_oauth_token_valid() {
    let json = format!(
        r#"{{"claudeAiOauth": {{"accessToken": "{}"}}}}"#,
        TEST_ANTHROPIC_OAUTH_BASIC
    );
    let token = parse_oauth_access_token(&json);
    assert_eq!(token, Some(TEST_ANTHROPIC_OAUTH_BASIC.to_string()));
}

#[test]
fn parse_oauth_token_missing_access_token() {
    let json = r#"{"claudeAiOauth": {}}"#;
    assert_eq!(parse_oauth_access_token(json), None);
}

#[test]
fn parse_oauth_token_missing_oauth_key() {
    let json = r#"{"someOtherKey": {"accessToken": "tok"}}"#;
    assert_eq!(parse_oauth_access_token(json), None);
}

#[test]
fn parse_oauth_token_invalid_json() {
    assert_eq!(parse_oauth_access_token("not json at all"), None);
}

#[test]
fn parse_oauth_token_empty_string() {
    assert_eq!(parse_oauth_access_token(""), None);
}

#[test]
fn parse_oauth_token_nested_extra_fields() {
    let json = format!(
        r#"{{
        "claudeAiOauth": {{
            "accessToken": "{}",
            "refreshToken": "rt-abc",
            "expiresAt": 1700000000
        }}
    }}"#,
        TEST_ANTHROPIC_OAUTH_NESTED
    );
    assert_eq!(
        parse_oauth_access_token(&json),
        Some(TEST_ANTHROPIC_OAUTH_NESTED.to_string())
    );
}

#[test]
fn parse_oauth_token_access_token_is_not_string() {
    let json = r#"{"claudeAiOauth": {"accessToken": 12345}}"#;
    assert_eq!(parse_oauth_access_token(json), None);
}

#[test]
fn parse_oauth_token_rejects_invalid_prefix() {
    let json = r#"{"claudeAiOauth": {"accessToken": "not-an-oauth-token"}}"#;
    assert_eq!(parse_oauth_access_token(json), None);
}

// ── default_claude_code_allowed_tools ───────────────────────────

#[test]
fn default_allowed_tools_has_expected_count() {
    let tools = default_claude_code_allowed_tools();
    // 10 tools: Read, Write, Edit, Glob, Grep, NotebookEdit, Bash, Task, WebFetch, WebSearch
    assert_eq!(tools.len(), 10);
}

#[test]
fn default_allowed_tools_all_have_glob_pattern() {
    let tools = default_claude_code_allowed_tools();
    for tool in &tools {
        assert!(
            tool.ends_with("(*)"),
            "tool '{tool}' should end with '(*)' glob pattern"
        );
    }
}
