//! Unit tests for parsing channel capability schema files.

use crate::channels::wasm::schema::ChannelCapabilitiesFile;

#[test]
fn test_parse_minimal() {
    let json = r#"{
        "name": "test"
    }"#;
    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    assert_eq!(file.name, "test");
    assert_eq!(file.r#type, "channel");
}

#[test]
fn test_parse_full_slack_example() {
    let json = r#"{
        "type": "channel",
        "name": "slack",
        "description": "Slack Events API channel",
        "capabilities": {
            "http": {
                "allowlist": [
                    { "host": "slack.com", "path_prefix": "/api/" }
                ],
                "credentials": {
                    "slack_bot": {
                        "secret_name": "slack_bot_token",
                        "location": { "type": "bearer" },
                        "host_patterns": ["slack.com"]
                    }
                },
                "rate_limit": { "requests_per_minute": 50, "requests_per_hour": 1000 }
            },
            "secrets": { "allowed_names": ["slack_*"] },
            "channel": {
                "allowed_paths": ["/webhook/slack"],
                "allow_polling": false,
                "emit_rate_limit": { "messages_per_minute": 100, "messages_per_hour": 5000 }
            }
        },
        "config": {
            "signing_secret_name": "slack_signing_secret"
        }
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    assert_eq!(file.name, "slack");
    assert_eq!(
        file.description,
        Some("Slack Events API channel".to_string())
    );

    let caps = file.to_capabilities();
    assert!(caps.is_path_allowed("/webhook/slack"));
    assert!(!caps.allow_polling);
    assert_eq!(caps.workspace_prefix, "channels/slack/");

    // Check tool capabilities were parsed
    assert!(caps.tool_capabilities.http.is_some());
    assert!(caps.tool_capabilities.secrets.is_some());

    // Check config
    let config_json = file.config_json();
    assert!(config_json.contains("signing_secret_name"));
}

#[test]
fn test_parse_with_polling() {
    let json = r#"{
        "name": "telegram",
        "capabilities": {
            "channel": {
                "allowed_paths": [],
                "allow_polling": true,
                "min_poll_interval_ms": 60000
            }
        }
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    let caps = file.to_capabilities();

    assert!(caps.allow_polling);
    assert_eq!(caps.min_poll_interval_ms, 60000);
}

#[test]
fn test_min_poll_interval_enforced() {
    let json = r#"{
        "name": "test",
        "capabilities": {
            "channel": {
                "allow_polling": true,
                "min_poll_interval_ms": 1000
            }
        }
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    let caps = file.to_capabilities();

    // Should be clamped to minimum
    assert_eq!(caps.min_poll_interval_ms, 30000);
}

#[test]
fn test_workspace_prefix_override() {
    let json = r#"{
        "name": "custom",
        "capabilities": {
            "channel": {
                "workspace_prefix": "integrations/custom/"
            }
        }
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    let caps = file.to_capabilities();

    assert_eq!(caps.workspace_prefix, "integrations/custom/");
}

#[test]
fn test_emit_rate_limit() {
    let json = r#"{
        "name": "test",
        "capabilities": {
            "channel": {
                "emit_rate_limit": {
                    "messages_per_minute": 50,
                    "messages_per_hour": 1000
                }
            }
        }
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    let caps = file.to_capabilities();

    assert_eq!(caps.emit_rate_limit.messages_per_minute, 50);
    assert_eq!(caps.emit_rate_limit.messages_per_hour, 1000);
}

#[test]
fn test_webhook_schema() {
    let json = r#"{
        "name": "telegram",
        "capabilities": {
            "channel": {
                "allowed_paths": ["/webhook/telegram"],
                "webhook": {
                    "secret_header": "X-Telegram-Bot-Api-Secret-Token",
                    "secret_name": "telegram_webhook_secret"
                }
            }
        }
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    assert_eq!(
        file.webhook_secret_header(),
        Some("X-Telegram-Bot-Api-Secret-Token")
    );
    assert_eq!(file.webhook_secret_name(), "telegram_webhook_secret");
}

#[test]
fn test_webhook_secret_name_default() {
    let json = r#"{
        "name": "mybot",
        "capabilities": {}
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    assert_eq!(file.webhook_secret_header(), None);
    assert_eq!(file.webhook_secret_name(), "mybot_webhook_secret");
}

#[test]
fn test_setup_schema() {
    let json = r#"{
        "name": "telegram",
        "setup": {
            "required_secrets": [
                {
                    "name": "telegram_bot_token",
                    "prompt": "Enter your Telegram Bot Token",
                    "validation": "^[0-9]+:[A-Za-z0-9_-]+$"
                },
                {
                    "name": "telegram_webhook_secret",
                    "prompt": "Webhook secret (leave empty to auto-generate)",
                    "optional": true,
                    "auto_generate": { "length": 64 }
                }
            ],
            "validation_endpoint": "https://api.telegram.org/bot{telegram_bot_token}/getMe"
        }
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    assert_eq!(file.setup.required_secrets.len(), 2);
    assert_eq!(file.setup.required_secrets[0].name, "telegram_bot_token");
    assert!(!file.setup.required_secrets[0].optional);
    assert!(file.setup.required_secrets[1].optional);
    assert_eq!(
        file.setup.required_secrets[1]
            .auto_generate
            .as_ref()
            .unwrap()
            .length,
        64
    );
}

// ── Category 5: Discord Capabilities Setup & Configuration ──────────

#[test]
fn test_validate_channel_short_prompt() {
    // prompt < 30 chars — should not panic
    let json = r#"{
        "name": "test-channel",
        "setup": {
            "required_secrets": [
                { "name": "bot_token", "prompt": "Bot token" }
            ],
            "setup_url": "https://example.com"
        }
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    // Should not panic; warning emitted for short prompt
    file.validate();
}

#[test]
fn test_validate_channel_missing_setup_url() {
    // required_secrets without setup_url — should not panic
    let json = r#"{
        "name": "test-channel",
        "setup": {
            "required_secrets": [
                {
                    "name": "bot_token",
                    "prompt": "Enter your bot token from the developer portal settings"
                }
            ]
        }
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    // Should not panic; warning emitted for missing setup_url
    file.validate();
}

#[test]
fn test_validate_clean_channel() {
    // Well-configured channel — should not panic or warn
    let json = r#"{
        "name": "good-channel",
        "setup": {
            "required_secrets": [
                {
                    "name": "bot_token",
                    "prompt": "Enter your bot token from https://example.com/bot-settings"
                }
            ],
            "setup_url": "https://example.com/bot-settings"
        }
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    // Should not panic and emits no warnings
    file.validate();
}

#[test]
fn test_discord_capabilities_has_public_key_secret() {
    let json = include_str!("../../../../channels-src/discord/discord.capabilities.json");
    let file = ChannelCapabilitiesFile::from_json(json).unwrap();

    let secret_names: Vec<&str> = file
        .setup
        .required_secrets
        .iter()
        .map(|s| s.name.as_str())
        .collect();

    assert!(
        secret_names.contains(&"discord_public_key"),
        "discord.capabilities.json must include discord_public_key in setup.required_secrets, \
         found: {:?}",
        secret_names
    );
}

#[test]
fn test_webhook_schema_signature_key_secret_name() {
    let json = r#"{
        "name": "discord",
        "capabilities": {
            "channel": {
                "allowed_paths": ["/webhook/discord"],
                "webhook": {
                    "signature_key_secret_name": "discord_public_key"
                }
            }
        }
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    assert_eq!(file.signature_key_secret_name(), Some("discord_public_key"));
}

#[test]
fn test_signature_key_secret_name_none_when_missing() {
    let json = r#"{
        "name": "telegram",
        "capabilities": {
            "channel": {
                "allowed_paths": ["/webhook/telegram"],
                "webhook": {
                    "secret_header": "X-Telegram-Bot-Api-Secret-Token"
                }
            }
        }
    }"#;

    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    assert_eq!(file.signature_key_secret_name(), None);
}

#[test]
fn test_discord_capabilities_signature_key() {
    let json = include_str!("../../../../channels-src/discord/discord.capabilities.json");
    let file = ChannelCapabilitiesFile::from_json(json).unwrap();
    assert_eq!(
        file.signature_key_secret_name(),
        Some("discord_public_key"),
        "discord.capabilities.json must declare signature_key_secret_name"
    );
}

#[test]
fn test_discord_capabilities_secrets_allowlist() {
    let json = include_str!("../../../../channels-src/discord/discord.capabilities.json");
    let file = ChannelCapabilitiesFile::from_json(json).unwrap();

    let caps = file.to_capabilities();
    let secrets_caps = caps
        .tool_capabilities
        .secrets
        .expect("Discord should have secrets capability");

    assert!(
        secrets_caps.is_allowed("discord_public_key"),
        "discord_public_key must be in the secrets allowlist"
    );
}
