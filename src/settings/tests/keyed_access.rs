//! Tests for DB-map round-trips and keyed get/set/reset/list access.

use crate::settings::*;

#[test]
fn test_db_map_round_trip() {
    let settings = Settings {
        selected_model: Some("claude-3-5-sonnet-20241022".to_string()),
        ..Default::default()
    };

    let map = settings.to_db_map();
    let restored = Settings::from_db_map(&map);
    assert_eq!(
        restored.selected_model,
        Some("claude-3-5-sonnet-20241022".to_string())
    );
}

#[test]
fn test_get_setting() {
    let settings = Settings::default();

    assert_eq!(settings.get("agent.name"), Some("ironclaw".to_string()));
    assert_eq!(
        settings.get("agent.max_parallel_jobs"),
        Some("5".to_string())
    );
    assert_eq!(settings.get("heartbeat.enabled"), Some("false".to_string()));
    assert_eq!(settings.get("nonexistent"), None);
}

#[test]
fn test_set_setting() {
    let mut settings = Settings::default();

    settings.set("agent.name", "mybot").unwrap();
    assert_eq!(settings.agent.name, "mybot");

    settings.set("agent.max_parallel_jobs", "10").unwrap();
    assert_eq!(settings.agent.max_parallel_jobs, 10);

    settings.set("heartbeat.enabled", "true").unwrap();
    assert!(settings.heartbeat.enabled);
}

#[test]
fn test_reset_setting() {
    let mut settings = Settings::default();

    settings.agent.name = "custom".to_string();
    settings.reset("agent.name").unwrap();
    assert_eq!(settings.agent.name, "ironclaw");
}

#[test]
fn test_list_settings() {
    let settings = Settings::default();
    let list = settings.list();

    // Check some expected entries
    assert!(list.iter().any(|(k, _)| k == "agent.name"));
    assert!(list.iter().any(|(k, _)| k == "heartbeat.enabled"));
    assert!(list.iter().any(|(k, _)| k == "onboard_completed"));
}

#[test]
fn test_key_source_serialization() {
    let settings = Settings {
        secrets_master_key_source: KeySource::Keychain,
        ..Default::default()
    };

    let json = serde_json::to_string(&settings).unwrap();
    assert!(json.contains("\"keychain\""));

    let loaded: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded.secrets_master_key_source, KeySource::Keychain);
}

#[test]
fn test_embeddings_defaults() {
    let settings = Settings::default();
    assert!(!settings.embeddings.enabled);
    assert_eq!(settings.embeddings.provider, "nearai");
    assert_eq!(settings.embeddings.model, "text-embedding-3-small");
}

#[test]
fn test_wasm_channel_owner_ids_db_round_trip() {
    let mut settings = Settings::default();
    settings
        .channels
        .wasm_channel_owner_ids
        .insert("telegram".to_string(), 123456789);

    let map = settings.to_db_map();
    let restored = Settings::from_db_map(&map);
    assert_eq!(
        restored.channels.wasm_channel_owner_ids.get("telegram"),
        Some(&123456789)
    );
}

#[test]
fn test_wasm_channel_owner_ids_default_empty() {
    let settings = Settings::default();
    assert!(settings.channels.wasm_channel_owner_ids.is_empty());
}

#[test]
fn test_wasm_channel_owner_ids_via_set() {
    let mut settings = Settings::default();
    settings
        .set("channels.wasm_channel_owner_ids.telegram", "987654321")
        .unwrap();
    assert_eq!(
        settings.channels.wasm_channel_owner_ids.get("telegram"),
        Some(&987654321)
    );
}

#[test]
fn test_llm_backend_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");

    let settings = Settings {
        llm_backend: Some("anthropic".to_string()),
        ollama_base_url: Some("http://localhost:11434".to_string()),
        openai_compatible_base_url: Some("http://my-vllm:8000/v1".to_string()),
        ..Default::default()
    };
    let json = serde_json::to_string_pretty(&settings).unwrap();
    ambient_fs::write(&path, json).unwrap();

    let loaded = Settings::load_from(&path);
    assert_eq!(loaded.llm_backend, Some("anthropic".to_string()));
    assert_eq!(
        loaded.ollama_base_url,
        Some("http://localhost:11434".to_string())
    );
    assert_eq!(
        loaded.openai_compatible_base_url,
        Some("http://my-vllm:8000/v1".to_string())
    );
}

#[test]
fn test_openai_compatible_db_map_round_trip() {
    let settings = Settings {
        llm_backend: Some("openai_compatible".to_string()),
        openai_compatible_base_url: Some("http://my-vllm:8000/v1".to_string()),
        embeddings: EmbeddingsSettings {
            enabled: false,
            ..Default::default()
        },
        ..Default::default()
    };

    let map = settings.to_db_map();
    let restored = Settings::from_db_map(&map);

    assert_eq!(
        restored.llm_backend,
        Some("openai_compatible".to_string()),
        "llm_backend must survive DB round-trip"
    );
    assert_eq!(
        restored.openai_compatible_base_url,
        Some("http://my-vllm:8000/v1".to_string()),
        "openai_compatible_base_url must survive DB round-trip"
    );
    assert!(
        !restored.embeddings.enabled,
        "embeddings.enabled=false must survive DB round-trip"
    );
}

#[test]
fn tunnel_settings_round_trip() {
    let settings = Settings {
        tunnel: TunnelSettings {
            provider: Some("ngrok".to_string()),
            ngrok_token: Some("tok_abc123".to_string()),
            ngrok_domain: Some("my.ngrok.dev".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };

    // JSON round-trip
    let json = serde_json::to_string(&settings).unwrap();
    let restored: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.tunnel.provider, Some("ngrok".to_string()));
    assert_eq!(restored.tunnel.ngrok_token, Some("tok_abc123".to_string()));
    assert_eq!(
        restored.tunnel.ngrok_domain,
        Some("my.ngrok.dev".to_string())
    );
    assert!(restored.tunnel.public_url.is_none());

    // DB map round-trip
    let map = settings.to_db_map();
    let from_db = Settings::from_db_map(&map);
    assert_eq!(from_db.tunnel.provider, Some("ngrok".to_string()));
    assert_eq!(from_db.tunnel.ngrok_token, Some("tok_abc123".to_string()));

    // get/set round-trip
    let mut s = Settings::default();
    s.set("tunnel.provider", "cloudflare").unwrap();
    s.set("tunnel.cf_token", "cf_tok_xyz").unwrap();
    s.set("tunnel.ts_funnel", "true").unwrap();
    assert_eq!(s.tunnel.provider, Some("cloudflare".to_string()));
    assert_eq!(s.tunnel.cf_token, Some("cf_tok_xyz".to_string()));
    assert!(s.tunnel.ts_funnel);
}
