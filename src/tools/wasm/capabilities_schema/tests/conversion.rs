//! Tests for runtime conversion, nested-wrapper resolution, and validation
//! warnings.

use crate::tools::wasm::capabilities_schema::CapabilitiesFile;

#[test]
fn test_to_capabilities() {
    let json = r#"{
        "http": {
            "allowlist": [{ "host": "api.slack.com", "path_prefix": "/api/" }],
            "rate_limit": { "requests_per_minute": 50, "requests_per_hour": 500 }
        },
        "secrets": {
            "allowed_names": ["slack_token"]
        }
    }"#;

    let file = CapabilitiesFile::from_json(json).unwrap();
    let caps = file.to_capabilities();

    assert!(caps.http.is_some());
    let http = caps.http.unwrap();
    assert_eq!(http.allowlist.len(), 1);
    assert_eq!(http.rate_limit.requests_per_minute, 50);

    assert!(caps.secrets.is_some());
    let secrets = caps.secrets.unwrap();
    assert!(secrets.is_allowed("slack_token"));
}

#[test]
fn test_full_slack_example() {
    let json = r#"{
        "http": {
            "allowlist": [
                { "host": "slack.com", "path_prefix": "/api/", "methods": ["GET", "POST"] }
            ],
            "credentials": {
                "slack_bot_token": {
                    "secret_name": "slack_bot_token",
                    "location": { "type": "bearer" },
                    "host_patterns": ["slack.com"]
                }
            },
            "rate_limit": { "requests_per_minute": 50, "requests_per_hour": 1000 }
        },
        "secrets": {
            "allowed_names": ["slack_bot_token"]
        }
    }"#;

    let file = CapabilitiesFile::from_json(json).unwrap();
    let caps = file.to_capabilities();

    let http = caps.http.unwrap();
    assert_eq!(http.allowlist[0].host, "slack.com");
    assert!(http.credentials.contains_key("slack_bot_token"));

    let secrets = caps.secrets.unwrap();
    assert!(secrets.is_allowed("slack_bot_token"));
}

// ── resolve_nested tests ──────────────────────────────────────────

#[test]
fn test_resolve_nested_outer_takes_precedence() {
    // Outer http should win over inner http
    let json = r#"{
        "http": {
            "allowlist": [{ "host": "outer.example.com" }]
        },
        "capabilities": {
            "http": {
                "allowlist": [{ "host": "inner.example.com" }]
            }
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let http = caps.http.unwrap();
    assert_eq!(
        http.allowlist[0].host, "outer.example.com",
        "Outer http should take precedence over inner"
    );
}

#[test]
fn test_resolve_nested_doubly_nested() {
    // capabilities.capabilities.http should resolve to top-level
    let json = r#"{
        "capabilities": {
            "capabilities": {
                "http": {
                    "allowlist": [{ "host": "deep.example.com" }]
                }
            }
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let http = caps.http.unwrap();
    assert_eq!(
        http.allowlist[0].host, "deep.example.com",
        "Doubly-nested capabilities should be resolved"
    );
}

#[test]
fn test_resolve_nested_all_fields_promoted() {
    // Inner has secrets, workspace, and auth — all should be promoted
    let json = r#"{
        "capabilities": {
            "secrets": {
                "allowed_names": ["my_secret"]
            },
            "workspace": {
                "allowed_prefixes": ["data/"]
            },
            "auth": {
                "secret_name": "my_auth_token"
            }
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    assert!(caps.secrets.is_some(), "secrets should be promoted");
    assert!(caps.workspace.is_some(), "workspace should be promoted");
    assert!(caps.auth.is_some(), "auth should be promoted");

    assert_eq!(caps.secrets.unwrap().allowed_names, vec!["my_secret"]);
    assert_eq!(caps.workspace.unwrap().allowed_prefixes, vec!["data/"]);
    assert_eq!(caps.auth.unwrap().secret_name, "my_auth_token");
}

#[test]
fn test_resolve_nested_setup_promoted() {
    // setup inside capabilities wrapper should be promoted to top level
    let json = r#"{
        "capabilities": {
            "setup": {
                "required_secrets": [
                    { "name": "my_secret", "prompt": "Enter secret" }
                ]
            }
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    assert!(
        caps.setup.is_some(),
        "setup should be promoted from inner capabilities"
    );
    assert_eq!(caps.setup.unwrap().required_secrets[0].name, "my_secret");
}

#[test]
fn test_validate_setup_without_auth_warns() {
    // setup.required_secrets with no auth section — should not panic
    let json = r#"{
        "setup": {
            "required_secrets": [
                { "name": "api_key", "prompt": "Enter your API key from the provider dashboard settings page" }
            ]
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    // Should not panic; warning is emitted via tracing
    caps.validate("test-tool");
}

#[test]
fn test_validate_manual_auth_missing_fields() {
    // auth without OAuth, missing setup_url and instructions
    let json = r#"{
        "auth": {
            "secret_name": "my_api_key"
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    // Should not panic; warnings emitted for missing setup_url and instructions
    caps.validate("test-tool");
}

#[test]
fn test_validate_clean_tool() {
    // Well-configured tool with auth, setup_url, instructions, and good prompts
    let json = r#"{
        "auth": {
            "secret_name": "my_api_key",
            "setup_url": "https://example.com/api-keys",
            "instructions": "Go to example.com/api-keys and create a new key"
        },
        "setup": {
            "required_secrets": [
                {
                    "name": "my_api_key",
                    "prompt": "Enter your API key from https://example.com/api-keys"
                }
            ]
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    // Should not panic and emits no warnings (has auth, setup_url, instructions, long prompt)
    caps.validate("clean-tool");
}

#[test]
fn test_resolve_nested_empty_capabilities_noop() {
    // Empty inner capabilities should not clobber outer http
    let json = r#"{
        "http": {
            "allowlist": [{ "host": "preserved.example.com" }]
        },
        "capabilities": {}
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let http = caps.http.unwrap();
    assert_eq!(
        http.allowlist[0].host, "preserved.example.com",
        "Empty inner capabilities should not clobber outer http"
    );
}
