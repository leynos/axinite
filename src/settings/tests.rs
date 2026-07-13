//! Unit tests for settings persistence, access, and defaults.

//! Unit tests for settings round-trips and keyed access.

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
fn toml_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    let mut settings = Settings::default();
    settings.agent.name = "toml-bot".to_string();
    settings.heartbeat.enabled = true;
    settings.heartbeat.interval_secs = 900;

    settings.save_toml(&path).unwrap();
    let loaded = Settings::load_toml(&path).unwrap().unwrap();

    assert_eq!(loaded.agent.name, "toml-bot");
    assert!(loaded.heartbeat.enabled);
    assert_eq!(loaded.heartbeat.interval_secs, 900);
}

/// Regression test: /model command must persist selected_model to TOML config.
/// Prior to the fix, `set_model()` only changed the in-memory provider and the
/// choice was lost on restart.
#[test]
fn toml_selected_model_update_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    // Start with a config that has a different model.
    let settings = Settings {
        selected_model: Some("old-model".to_string()),
        ..Default::default()
    };
    settings.save_toml(&path).unwrap();

    // Simulate what persist_selected_model does: load, update, save.
    let mut loaded = Settings::load_toml(&path).unwrap().unwrap();
    loaded.selected_model = Some("new-model".to_string());
    loaded.save_toml(&path).unwrap();

    // Verify the change survived a reload.
    let reloaded = Settings::load_toml(&path).unwrap().unwrap();
    assert_eq!(reloaded.selected_model, Some("new-model".to_string()));
}

#[test]
fn toml_missing_file_returns_none() {
    let result = Settings::load_toml(std::path::Path::new("/tmp/nonexistent_config.toml"));
    assert!(result.unwrap().is_none());
}

#[test]
fn toml_invalid_content_returns_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.toml");
    ambient_fs::write(&path, "this is not valid toml [[[").unwrap();

    let result = Settings::load_toml(&path);
    assert!(result.is_err());
}

#[test]
fn toml_partial_config_uses_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("partial.toml");

    // Only set agent name, everything else should be default
    ambient_fs::write(&path, "[agent]\nname = \"partial-bot\"\n").unwrap();

    let loaded = Settings::load_toml(&path).unwrap().unwrap();
    assert_eq!(loaded.agent.name, "partial-bot");
    // Defaults preserved
    assert_eq!(loaded.agent.max_parallel_jobs, 5);
    assert!(!loaded.heartbeat.enabled);
}

#[test]
fn toml_header_comment_present() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");

    Settings::default().save_toml(&path).unwrap();
    let content = ambient_fs::read_to_string(&path).unwrap();

    assert!(content.starts_with("# IronClaw configuration file."));
    assert!(content.contains("[agent]"));
    assert!(content.contains("[heartbeat]"));
}

#[test]
fn merge_only_overrides_non_default_values() {
    let mut base = Settings::default();
    base.agent.name = "from-db".to_string();
    base.heartbeat.interval_secs = 600;

    let mut toml_overlay = Settings::default();
    toml_overlay.agent.name = "from-toml".to_string();

    base.merge_from(&toml_overlay);

    assert_eq!(base.agent.name, "from-toml");
    assert_eq!(base.heartbeat.interval_secs, 600);
}

#[test]
fn merge_preserves_base_when_overlay_is_default() {
    let mut base = Settings::default();
    base.agent.name = "custom-name".to_string();
    base.heartbeat.enabled = true;

    let overlay = Settings::default();
    base.merge_from(&overlay);

    assert_eq!(base.agent.name, "custom-name");
    assert!(base.heartbeat.enabled);
}

#[test]
fn toml_creates_parent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested").join("deep").join("config.toml");

    Settings::default().save_toml(&path).unwrap();
    assert!(path.exists());
}

#[test]
fn default_toml_path_under_ironclaw() {
    let path = Settings::default_toml_path();
    assert!(path.to_string_lossy().contains(".ironclaw"));
    assert!(path.to_string_lossy().ends_with("config.toml"));
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

/// Simulates the wizard recovery scenario:
///
/// 1. A prior partial run saved steps 1-4 to the DB
/// 2. User re-runs the wizard, Step 1 sets a new database_url
/// 3. Prior settings are loaded from the DB
/// 4. Step 1's fresh choices must win over stale DB values
///
/// This tests the ordering: load DB → merge_from(step1_overrides).
#[test]
fn wizard_recovery_step1_overrides_stale_db() {
    // Simulate prior partial run (steps 1-4 completed):
    let prior_run = Settings {
        database_backend: Some("postgres".to_string()),
        database_url: Some("postgres://old-host/ironclaw".to_string()),
        llm_backend: Some("anthropic".to_string()),
        selected_model: Some("claude-sonnet-4-5".to_string()),
        embeddings: EmbeddingsSettings {
            enabled: true,
            provider: "openai".to_string(),
            ..Default::default()
        },
        ..Default::default()
    };

    // Save to DB and reload (simulates persistence round-trip)
    let db_map = prior_run.to_db_map();
    let from_db = Settings::from_db_map(&db_map);

    // Step 1 of the new wizard run: user enters a NEW database_url
    let step1_settings = Settings {
        database_backend: Some("postgres".to_string()),
        database_url: Some("postgres://new-host/ironclaw".to_string()),
        ..Settings::default()
    };

    // Wizard flow: load DB → merge_from(step1_overrides)
    let mut current = step1_settings.clone();
    // try_load_existing_settings: merge DB into current
    current.merge_from(&from_db);
    // Re-apply Step 1 choices on top
    current.merge_from(&step1_settings);

    // Step 1's fresh database_url wins over stale DB value
    assert_eq!(
        current.database_url,
        Some("postgres://new-host/ironclaw".to_string()),
        "Step 1 fresh choice must override stale DB value"
    );

    // Prior run's steps 2-4 settings are preserved
    assert_eq!(
        current.llm_backend,
        Some("anthropic".to_string()),
        "Prior run's LLM backend must be recovered"
    );
    assert_eq!(
        current.selected_model,
        Some("claude-sonnet-4-5".to_string()),
        "Prior run's model must be recovered"
    );
    assert!(
        current.embeddings.enabled,
        "Prior run's embeddings setting must be recovered"
    );
}

/// Verifies that persisting defaults doesn't clobber prior settings
/// when the merge ordering is correct.
#[test]
fn wizard_recovery_defaults_dont_clobber_prior() {
    // Prior run saved non-default settings
    let prior_run = Settings {
        llm_backend: Some("openai".to_string()),
        selected_model: Some("gpt-4o".to_string()),
        heartbeat: HeartbeatSettings {
            enabled: true,
            interval_secs: 900,
            ..Default::default()
        },
        ..Default::default()
    };
    let db_map = prior_run.to_db_map();
    let from_db = Settings::from_db_map(&db_map);

    // New wizard run: Step 1 only sets DB fields (rest is default)
    let step1 = Settings {
        database_backend: Some("libsql".to_string()),
        ..Default::default()
    };

    // Correct merge ordering
    let mut current = step1.clone();
    current.merge_from(&from_db);
    current.merge_from(&step1);

    // Prior settings preserved (Step 1 doesn't touch these)
    assert_eq!(current.llm_backend, Some("openai".to_string()));
    assert_eq!(current.selected_model, Some("gpt-4o".to_string()));
    assert!(current.heartbeat.enabled);
    assert_eq!(current.heartbeat.interval_secs, 900);

    // Step 1's choice applied
    assert_eq!(current.database_backend, Some("libsql".to_string()));
}

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
