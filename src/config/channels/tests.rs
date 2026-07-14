//! Unit tests for channel configuration parsing and defaults.

use crate::config::EnvContext;
use crate::config::channels::*;
use crate::settings::Settings;

#[test]
fn cli_config_fields() {
    let cfg = CliConfig { enabled: true };
    assert!(cfg.enabled);

    let disabled = CliConfig { enabled: false };
    assert!(!disabled.enabled);
}

#[test]
fn http_config_fields() {
    let cfg = HttpConfig {
        host: "0.0.0.0".to_string(),
        port: 8080,
        webhook_secret: None,
        user_id: "http".to_string(),
    };
    assert_eq!(cfg.host, "0.0.0.0");
    assert_eq!(cfg.port, 8080);
    assert!(cfg.webhook_secret.is_none());
    assert_eq!(cfg.user_id, "http");
}

#[test]
fn http_config_with_secret() {
    let cfg = HttpConfig {
        host: "127.0.0.1".to_string(),
        port: 9090,
        webhook_secret: Some(secrecy::SecretString::from("s3cret".to_string())),
        user_id: "webhook-bot".to_string(),
    };
    assert!(cfg.webhook_secret.is_some());
    assert_eq!(cfg.port, 9090);
}

#[test]
fn resolve_from_uses_settings_http_config_when_env_absent() {
    let settings = Settings {
        channels: crate::settings::ChannelSettings {
            http_enabled: true,
            http_host: Some("127.0.0.1".to_string()),
            http_port: Some(9090),
            ..Default::default()
        },
        ..Default::default()
    };

    let cfg = ChannelsConfig::resolve_from(&EnvContext::default(), &settings)
        .expect("settings-backed HTTP config should resolve");
    let http = cfg.http.expect("HTTP config should be present");
    assert_eq!(http.host, "127.0.0.1");
    assert_eq!(http.port, 9090);
}

#[test]
fn resolve_from_keeps_http_disabled_when_only_settings_host_and_port_exist() {
    let settings = Settings {
        channels: crate::settings::ChannelSettings {
            http_enabled: false,
            http_host: Some("127.0.0.1".to_string()),
            http_port: Some(9090),
            ..Default::default()
        },
        ..Default::default()
    };

    let cfg = ChannelsConfig::resolve_from(&EnvContext::default(), &settings)
        .expect("disabled HTTP without env overrides should resolve");
    assert!(
        cfg.http.is_none(),
        "persisted host/port must not enable HTTP"
    );
}

#[test]
fn resolve_from_uses_settings_signal_config_when_env_absent() {
    let settings = Settings {
        channels: crate::settings::ChannelSettings {
            signal_enabled: true,
            signal_http_url: Some("http://127.0.0.1:8080".to_string()),
            signal_account: Some("+15551234567".to_string()),
            signal_dm_policy: Some("open".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    let cfg = ChannelsConfig::resolve_from(&EnvContext::default(), &settings)
        .expect("settings-backed Signal config should resolve");
    let signal = cfg.signal.expect("Signal config should be present");
    assert_eq!(signal.http_url, "http://127.0.0.1:8080");
    assert_eq!(signal.account, "+15551234567");
    assert_eq!(signal.allow_from, vec!["+15551234567".to_string()]);
    assert_eq!(signal.dm_policy, "open");
}

#[test]
fn gateway_config_fields() {
    let cfg = GatewayConfig {
        host: "127.0.0.1".to_string(),
        port: 3000,
        auth_token: Some("tok-abc".to_string()),
        user_id: "default".to_string(),
    };
    assert_eq!(cfg.host, "127.0.0.1");
    assert_eq!(cfg.port, 3000);
    assert_eq!(cfg.auth_token.as_deref(), Some("tok-abc"));
    assert_eq!(cfg.user_id, "default");
}

#[test]
fn gateway_config_no_auth_token() {
    let cfg = GatewayConfig {
        host: "0.0.0.0".to_string(),
        port: 3001,
        auth_token: None,
        user_id: "anon".to_string(),
    };
    assert!(cfg.auth_token.is_none());
}

#[test]
fn signal_config_fields_and_defaults() {
    let cfg = SignalConfig {
        http_url: "http://127.0.0.1:8080".to_string(),
        account: "+1234567890".to_string(),
        allow_from: vec!["+1234567890".to_string()],
        allow_from_groups: vec![],
        dm_policy: "pairing".to_string(),
        group_policy: "allowlist".to_string(),
        group_allow_from: vec![],
        ignore_attachments: false,
        ignore_stories: true,
    };
    assert_eq!(cfg.http_url, "http://127.0.0.1:8080");
    assert_eq!(cfg.account, "+1234567890");
    assert_eq!(cfg.allow_from, vec!["+1234567890"]);
    assert!(cfg.allow_from_groups.is_empty());
    assert_eq!(cfg.dm_policy, "pairing");
    assert_eq!(cfg.group_policy, "allowlist");
    assert!(cfg.group_allow_from.is_empty());
    assert!(!cfg.ignore_attachments);
    assert!(cfg.ignore_stories);
}

#[test]
fn signal_config_open_policies() {
    let cfg = SignalConfig {
        http_url: "http://localhost:7583".to_string(),
        account: "+0000000000".to_string(),
        allow_from: vec!["*".to_string()],
        allow_from_groups: vec!["*".to_string()],
        dm_policy: "open".to_string(),
        group_policy: "open".to_string(),
        group_allow_from: vec![],
        ignore_attachments: true,
        ignore_stories: false,
    };
    assert_eq!(cfg.allow_from, vec!["*"]);
    assert_eq!(cfg.allow_from_groups, vec!["*"]);
    assert_eq!(cfg.dm_policy, "open");
    assert_eq!(cfg.group_policy, "open");
    assert!(cfg.ignore_attachments);
    assert!(!cfg.ignore_stories);
}

#[test]
fn channels_config_fields() {
    let cfg = ChannelsConfig {
        cli: CliConfig { enabled: true },
        http: None,
        gateway: None,
        signal: None,
        wasm_channels_dir: PathBuf::from("/tmp/channels"),
        wasm_channels_enabled: true,
        wasm_channel_owner_ids: HashMap::new(),
    };
    assert!(cfg.cli.enabled);
    assert!(cfg.http.is_none());
    assert!(cfg.gateway.is_none());
    assert!(cfg.signal.is_none());
    assert_eq!(cfg.wasm_channels_dir, PathBuf::from("/tmp/channels"));
    assert!(cfg.wasm_channels_enabled);
    assert!(cfg.wasm_channel_owner_ids.is_empty());
}

#[test]
fn channels_config_with_owner_ids() {
    let mut ids = HashMap::new();
    ids.insert("telegram".to_string(), 12345_i64);
    ids.insert("slack".to_string(), 67890_i64);

    let cfg = ChannelsConfig {
        cli: CliConfig { enabled: false },
        http: None,
        gateway: None,
        signal: None,
        wasm_channels_dir: PathBuf::from("/opt/channels"),
        wasm_channels_enabled: false,
        wasm_channel_owner_ids: ids,
    };
    assert_eq!(cfg.wasm_channel_owner_ids.get("telegram"), Some(&12345));
    assert_eq!(cfg.wasm_channel_owner_ids.get("slack"), Some(&67890));
    assert!(!cfg.wasm_channels_enabled);
}

#[test]
fn default_channels_dir_ends_with_channels() {
    let dir = default_channels_dir();
    assert!(
        dir.ends_with("channels"),
        "expected path ending in 'channels', got: {dir:?}"
    );
}
