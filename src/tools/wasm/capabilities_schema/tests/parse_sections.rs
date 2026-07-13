//! Tests for parsing non-HTTP capability sections: secrets, tool invocation,
//! workspace, auth, and setup.

use crate::tools::wasm::capabilities_schema::CapabilitiesFile;

#[test]
fn test_parse_minimal() {
    let json = "{}";
    let caps = CapabilitiesFile::from_json(json).unwrap();
    assert!(caps.http.is_none());
    assert!(caps.secrets.is_none());
}

#[test]
fn test_parse_secrets_capability() {
    let json = r#"{
        "secrets": {
            "allowed_names": ["slack_*", "openai_key"]
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let secrets = caps.secrets.unwrap();
    assert_eq!(secrets.allowed_names, vec!["slack_*", "openai_key"]);
}

#[test]
fn test_parse_tool_invoke() {
    let json = r#"{
        "tool_invoke": {
            "aliases": {
                "search": "brave_search",
                "calc": "calculator"
            },
            "rate_limit": {
                "requests_per_minute": 10,
                "requests_per_hour": 100
            }
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let tool_invoke = caps.tool_invoke.unwrap();
    assert_eq!(
        tool_invoke.aliases.get("search"),
        Some(&"brave_search".to_string())
    );
    let rate = tool_invoke.rate_limit.unwrap();
    assert_eq!(rate.requests_per_minute, 10);
}

#[test]
fn test_parse_workspace() {
    let json = r#"{
        "workspace": {
            "allowed_prefixes": ["context/", "daily/"]
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let workspace = caps.workspace.unwrap();
    assert_eq!(workspace.allowed_prefixes, vec!["context/", "daily/"]);
}

#[test]
fn test_parse_auth_capability() {
    let json = r#"{
        "auth": {
            "secret_name": "notion_api_token",
            "display_name": "Notion",
            "instructions": "Create an integration at notion.so/my-integrations",
            "setup_url": "https://www.notion.so/my-integrations",
            "token_hint": "Starts with 'secret_' or 'ntn_'",
            "env_var": "NOTION_TOKEN",
            "provider": "notion",
            "validation_endpoint": {
                "url": "https://api.notion.com/v1/users/me",
                "method": "GET",
                "success_status": 200
            }
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let auth = caps.auth.unwrap();
    assert_eq!(auth.secret_name, "notion_api_token");
    assert_eq!(auth.display_name, Some("Notion".to_string()));
    assert_eq!(auth.env_var, Some("NOTION_TOKEN".to_string()));
    assert_eq!(auth.provider, Some("notion".to_string()));

    let validation = auth.validation_endpoint.unwrap();
    assert_eq!(validation.url, "https://api.notion.com/v1/users/me");
    assert_eq!(validation.method, "GET");
    assert_eq!(validation.success_status, 200);
}

#[test]
fn test_parse_auth_minimal() {
    let json = r#"{
        "auth": {
            "secret_name": "my_api_key"
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let auth = caps.auth.unwrap();
    assert_eq!(auth.secret_name, "my_api_key");
    assert!(auth.display_name.is_none());
    assert!(auth.setup_url.is_none());
}

#[test]
fn test_parse_tool_setup_schema() {
    let json = r#"{
        "setup": {
            "required_secrets": [
                {
                    "name": "google_oauth_client_id",
                    "prompt": "Google OAuth Client ID"
                },
                {
                    "name": "google_oauth_client_secret",
                    "prompt": "Google OAuth Client Secret",
                    "optional": true
                }
            ]
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let setup = caps.setup.unwrap();
    assert_eq!(setup.required_secrets.len(), 2);
    assert_eq!(setup.required_secrets[0].name, "google_oauth_client_id");
    assert_eq!(setup.required_secrets[0].prompt, "Google OAuth Client ID");
    assert!(!setup.required_secrets[0].optional);
    assert_eq!(setup.required_secrets[1].name, "google_oauth_client_secret");
    assert!(setup.required_secrets[1].optional);
}
