//! QA Plan P1-1.2: comprehensive config round-trip agreement tests.

use crate::settings::*;

// === QA Plan P1 - 1.2: Config round-trip tests ===

#[test]
fn comprehensive_db_map_round_trip() {
    // Set a representative value in EVERY section and verify survival
    let settings = Settings {
        onboard_completed: true,
        database_backend: Some("libsql".to_string()),
        database_url: Some("postgres://host/db".to_string()),
        llm_backend: Some("anthropic".to_string()),
        selected_model: Some("claude-sonnet-4-5".to_string()),
        openai_compatible_base_url: Some("http://vllm:8000/v1".to_string()),
        secrets_master_key_source: KeySource::Keychain,
        embeddings: EmbeddingsSettings {
            enabled: true,
            provider: "nearai".to_string(),
            model: "text-embedding-3-large".to_string(),
        },
        tunnel: TunnelSettings {
            provider: Some("ngrok".to_string()),
            ngrok_token: Some("tok_xxx".to_string()),
            ..Default::default()
        },
        channels: ChannelSettings {
            http_enabled: true,
            http_port: Some(9090),
            wasm_channel_owner_ids: {
                let mut m = std::collections::HashMap::new();
                m.insert("telegram".to_string(), 12345);
                m
            },
            ..Default::default()
        },
        heartbeat: HeartbeatSettings {
            enabled: true,
            interval_secs: 900,
            ..Default::default()
        },
        agent: AgentSettings {
            name: "my-bot".to_string(),
            max_parallel_jobs: 10,
            ..Default::default()
        },
        ..Default::default()
    };

    let map = settings.to_db_map();
    let restored = Settings::from_db_map(&map);

    assert!(restored.onboard_completed, "onboard_completed lost");
    assert_eq!(
        restored.database_backend,
        Some("libsql".to_string()),
        "database_backend lost"
    );
    assert_eq!(
        restored.database_url,
        Some("postgres://host/db".to_string()),
        "database_url lost"
    );
    assert_eq!(
        restored.llm_backend,
        Some("anthropic".to_string()),
        "llm_backend lost"
    );
    assert_eq!(
        restored.selected_model,
        Some("claude-sonnet-4-5".to_string()),
        "selected_model lost"
    );
    assert_eq!(
        restored.openai_compatible_base_url,
        Some("http://vllm:8000/v1".to_string()),
        "openai_compatible_base_url lost"
    );
    assert_eq!(
        restored.secrets_master_key_source,
        KeySource::Keychain,
        "key_source lost"
    );
    assert!(restored.embeddings.enabled, "embeddings.enabled lost");
    assert_eq!(
        restored.embeddings.provider, "nearai",
        "embeddings.provider lost"
    );
    assert_eq!(
        restored.embeddings.model, "text-embedding-3-large",
        "embeddings.model lost"
    );
    assert_eq!(
        restored.tunnel.provider,
        Some("ngrok".to_string()),
        "tunnel.provider lost"
    );
    assert!(restored.channels.http_enabled, "http_enabled lost");
    assert_eq!(restored.channels.http_port, Some(9090), "http_port lost");
    assert_eq!(
        restored.channels.wasm_channel_owner_ids.get("telegram"),
        Some(&12345),
        "wasm_channel_owner_ids lost"
    );
    assert!(restored.heartbeat.enabled, "heartbeat.enabled lost");
    assert_eq!(
        restored.heartbeat.interval_secs, 900,
        "heartbeat.interval_secs lost"
    );
    assert_eq!(restored.agent.name, "my-bot", "agent.name lost");
    assert_eq!(
        restored.agent.max_parallel_jobs, 10,
        "agent.max_parallel_jobs lost"
    );
}

#[test]
fn toml_json_db_all_agree() {
    // A config that goes through all three formats should produce the same values
    let dir = tempfile::tempdir().unwrap();
    let toml_path = dir.path().join("config.toml");
    let json_path = dir.path().join("settings.json");

    let original = Settings {
        llm_backend: Some("ollama".to_string()),
        selected_model: Some("llama3".to_string()),
        heartbeat: HeartbeatSettings {
            enabled: true,
            interval_secs: 600,
            ..Default::default()
        },
        agent: AgentSettings {
            name: "round-trip-bot".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };

    // TOML round-trip
    original.save_toml(&toml_path).unwrap();
    let from_toml = Settings::load_toml(&toml_path).unwrap().unwrap();

    // JSON round-trip
    let json = serde_json::to_string_pretty(&original).unwrap();
    ambient_fs::write(&json_path, &json).unwrap();
    let from_json = Settings::load_from(&json_path);

    // DB map round-trip
    let db_map = original.to_db_map();
    let from_db = Settings::from_db_map(&db_map);

    // All three should agree on key values
    for (label, loaded) in [("TOML", &from_toml), ("JSON", &from_json), ("DB", &from_db)] {
        assert_eq!(
            loaded.llm_backend,
            Some("ollama".to_string()),
            "{label}: llm_backend"
        );
        assert_eq!(
            loaded.selected_model,
            Some("llama3".to_string()),
            "{label}: selected_model"
        );
        assert!(loaded.heartbeat.enabled, "{label}: heartbeat.enabled");
        assert_eq!(
            loaded.heartbeat.interval_secs, 600,
            "{label}: heartbeat.interval_secs"
        );
        assert_eq!(loaded.agent.name, "round-trip-bot", "{label}: agent.name");
    }
}

#[test]
fn set_get_round_trip_all_documented_paths() -> anyhow::Result<()> {
    use anyhow::Context as _;

    let mut settings = Settings::default();

    // Test set + get for each documented settings path
    let test_cases: Vec<(&str, &str)> = vec![
        ("agent.name", "test-agent"),
        ("agent.max_parallel_jobs", "8"),
        ("heartbeat.enabled", "true"),
        ("heartbeat.interval_secs", "300"),
        ("channels.http_enabled", "true"),
        ("channels.http_port", "8081"),
    ];

    for (path, value) in &test_cases {
        settings
            .set(path, value)
            .map_err(|e| anyhow::anyhow!("set({path}, {value}) failed: {e}"))?;
        let got = settings
            .get(path)
            .with_context(|| format!("get({path}) returned None after set"))?;
        assert_eq!(&got, value, "set/get round-trip failed for path '{path}'");
    }
    Ok(())
}

#[test]
fn option_string_fields_survive_db_round_trip_as_null() {
    // When an Option<String> field is None, it should be stored as null
    // and come back as None, not silently become Some("")
    let settings = Settings {
        database_url: None,
        llm_backend: None,
        selected_model: None,
        openai_compatible_base_url: None,
        ..Default::default()
    };

    let map = settings.to_db_map();
    let restored = Settings::from_db_map(&map);

    assert_eq!(
        restored.database_url, None,
        "None database_url should stay None"
    );
    assert_eq!(
        restored.llm_backend, None,
        "None llm_backend should stay None"
    );
    assert_eq!(
        restored.selected_model, None,
        "None selected_model should stay None"
    );
}
