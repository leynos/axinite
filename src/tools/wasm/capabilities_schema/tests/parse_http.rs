//! Tests for parsing HTTP capability sections: allowlists, credentials, and
//! credential injection locations.

use crate::tools::wasm::capabilities_schema::CapabilitiesFile;
use crate::tools::wasm::capabilities_schema::http::CredentialLocationSchema;

#[test]
fn test_parse_http_allowlist() {
    let json = r#"{
        "http": {
            "allowlist": [
                { "host": "api.slack.com", "path_prefix": "/api/", "methods": ["GET", "POST"] }
            ]
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let http = caps.http.unwrap();
    assert_eq!(http.allowlist.len(), 1);
    assert_eq!(http.allowlist[0].host, "api.slack.com");
    assert_eq!(http.allowlist[0].path_prefix, Some("/api/".to_string()));
    assert_eq!(http.allowlist[0].methods, vec!["GET", "POST"]);
}

#[test]
fn test_parse_credentials() {
    let json = r#"{
        "http": {
            "allowlist": [{ "host": "slack.com" }],
            "credentials": {
                "slack": {
                    "secret_name": "slack_bot_token",
                    "location": { "type": "bearer" },
                    "host_patterns": ["slack.com", "*.slack.com"]
                }
            }
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let http = caps.http.unwrap();
    assert_eq!(http.credentials.len(), 1);
    let cred = http.credentials.get("slack").unwrap();
    assert_eq!(cred.secret_name, "slack_bot_token");
    assert!(matches!(cred.location, CredentialLocationSchema::Bearer));
    assert_eq!(cred.host_patterns, vec!["slack.com", "*.slack.com"]);
}

#[test]
fn test_parse_custom_header_credential() {
    let json = r#"{
        "http": {
            "allowlist": [{ "host": "api.example.com" }],
            "credentials": {
                "api_key": {
                    "secret_name": "my_api_key",
                    "location": { "type": "header", "name": "X-API-Key", "prefix": "Key " },
                    "host_patterns": ["api.example.com"]
                }
            }
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let http = caps.http.unwrap();
    let cred = http.credentials.get("api_key").unwrap();
    match &cred.location {
        CredentialLocationSchema::Header { name, prefix } => {
            assert_eq!(name, "X-API-Key");
            assert_eq!(prefix, &Some("Key ".to_string()));
        }
        _ => panic!("Expected Header location"),
    }
}

#[test]
fn test_parse_url_path_credential() {
    let json = r#"{
        "http": {
            "allowlist": [{ "host": "api.telegram.org" }],
            "credentials": {
                "telegram_bot": {
                    "secret_name": "telegram_bot_token",
                    "location": {
                        "type": "url_path",
                        "placeholder": "{TELEGRAM_BOT_TOKEN}"
                    },
                    "host_patterns": ["api.telegram.org"]
                }
            }
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let http = caps.http.unwrap();
    let cred = http.credentials.get("telegram_bot").unwrap();
    match &cred.location {
        CredentialLocationSchema::UrlPath { placeholder } => {
            assert_eq!(placeholder, "{TELEGRAM_BOT_TOKEN}");
        }
        _ => panic!("Expected UrlPath location"),
    }
}

// ── Category 1: Header field name alias ─────────────────────────────

#[test]
fn test_header_location_with_name_field() {
    let json = r#"{
        "http": {
            "allowlist": [{ "host": "discord.com" }],
            "credentials": {
                "bot_token": {
                    "secret_name": "discord_bot_token",
                    "location": { "type": "header", "name": "Authorization", "prefix": "Bot " },
                    "host_patterns": ["discord.com"]
                }
            }
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let http = caps.http.unwrap();
    let cred = http.credentials.get("bot_token").unwrap();
    match &cred.location {
        CredentialLocationSchema::Header { name, prefix } => {
            assert_eq!(name, "Authorization");
            assert_eq!(prefix, &Some("Bot ".to_string()));
        }
        _ => panic!("Expected Header location"),
    }
}

#[test]
fn test_header_location_with_header_name_alias() {
    // Uses "header_name" instead of "name" — should parse via serde alias
    let json = r#"{
        "http": {
            "allowlist": [{ "host": "discord.com" }],
            "credentials": {
                "bot_token": {
                    "secret_name": "discord_bot_token",
                    "location": { "type": "header", "header_name": "Authorization", "prefix": "Bot " },
                    "host_patterns": ["discord.com"]
                }
            }
        }
    }"#;

    let caps = CapabilitiesFile::from_json(json).unwrap();
    let http = caps.http.unwrap();
    let cred = http.credentials.get("bot_token").unwrap();
    match &cred.location {
        CredentialLocationSchema::Header { name, prefix } => {
            assert_eq!(name, "Authorization");
            assert_eq!(prefix, &Some("Bot ".to_string()));
        }
        _ => panic!("Expected Header location"),
    }
}

#[test]
fn test_discord_capabilities_file_parses() {
    // Full Discord capabilities JSON — tests end-to-end parsing
    let json = r#"{
        "type": "channel",
        "name": "discord",
        "description": "Discord channel",
        "setup": {
            "required_secrets": [
                {
                    "name": "discord_bot_token",
                    "prompt": "Enter your Discord Bot Token",
                    "optional": false
                },
                {
                    "name": "discord_public_key",
                    "prompt": "Enter your Discord Public Key",
                    "optional": false
                }
            ]
        },
        "capabilities": {
            "http": {
                "allowlist": [{ "host": "discord.com", "path_prefix": "/api/v10" }],
                "credentials": {
                    "discord_bot_token": {
                        "secret_name": "discord_bot_token",
                        "location": { "type": "header", "name": "Authorization", "prefix": "Bot " },
                        "host_patterns": ["discord.com"]
                    }
                }
            }
        },
        "config": {
            "require_signature_verification": true
        }
    }"#;

    // This must not panic — parsing should succeed
    let caps = CapabilitiesFile::from_json(json).unwrap();
    let http = caps.http.unwrap();
    assert!(http.credentials.contains_key("discord_bot_token"));
}

#[test]
fn test_header_location_missing_name_fails() {
    // Neither "name" nor "header_name" provided — should fail
    let json = r#"{
        "http": {
            "allowlist": [{ "host": "example.com" }],
            "credentials": {
                "api_key": {
                    "secret_name": "my_key",
                    "location": { "type": "header", "prefix": "Key " },
                    "host_patterns": ["example.com"]
                }
            }
        }
    }"#;

    assert!(
        CapabilitiesFile::from_json(json).is_err(),
        "Header without name or header_name should fail deserialization"
    );
}
