//! Tests for wizard recovery merge ordering over stale DB settings.

use crate::settings::*;

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
        database_url: Some("postgres://old-host/axinite".to_string()),
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
        database_url: Some("postgres://new-host/axinite".to_string()),
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
        Some("postgres://new-host/axinite".to_string()),
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
