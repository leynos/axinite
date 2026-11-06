//! Unit tests for doctor diagnostic checks and result formatting.

use super::CheckResult;
use super::core_checks::{
    check_database_with_context, check_llm_config_with_context, check_settings_file,
    check_workspace_search_with_context,
};
use super::external_checks::check_binary;
use super::settings_for_context;
use super::subsystem_checks::{
    check_embeddings_with_context, check_routines_config_with_context, check_secrets,
};
use crate::config::EnvContext;
use crate::settings::Settings;

/// Verify that the database and workspace-search checks report the auto-detected libSQL backend.
#[tokio::test]
async fn database_and_search_checks_use_auto_detected_libsql_backend() {
    let db_file = tempfile::NamedTempFile::new().expect("create libSQL marker file");
    let context =
        crate::bootstrap::with_libsql_backend_autodetection(EnvContext::default(), db_file.path())
            .with_env("LIBSQL_PATH", db_file.path().to_string_lossy());

    match check_database_with_context(&context).await {
        CheckResult::Pass(message) => assert!(message.contains("libSQL database exists")),
        other => panic!(
            "expected libSQL database result, got: {}",
            format_result(&other)
        ),
    }
    match check_workspace_search_with_context(&context).await {
        CheckResult::Pass(message) => {
            assert!(message.contains("brute-force cosine"));
        }
        other => panic!(
            "expected libSQL search result, got: {}",
            format_result(&other)
        ),
    }
}

/// Verify that valid JSON with an invalid `Settings` field type fails the check.
///
/// Startup rejects such a file, so doctor must not report it as valid merely
/// because it parses as generic JSON.
#[test]
fn check_settings_file_rejects_invalid_field_types() {
    let dir = tempfile::tempdir().expect("create temp dir for settings check");
    ambient_fs::write(
        dir.path().join("settings.json"),
        r#"{"onboard_completed": "yes"}"#,
    )
    .expect("write settings.json with an invalid field type");
    let context = EnvContext::default().with_env(
        "AXINITE_BASE_DIR",
        dir.path().to_str().expect("UTF-8 temp path"),
    );

    match check_settings_file(&context) {
        CheckResult::Fail(message) => assert!(
            message.contains("settings.json is invalid"),
            "expected an invalid-settings message, got: {message}"
        ),
        other => panic!(
            "expected Fail for invalid settings, got: {}",
            format_result(&other)
        ),
    }
}

/// Doctor must read settings from the snapshot's base directory, not the
/// ambient one, so diagnostics describe the deployment being inspected.
#[test]
fn settings_for_context_uses_the_snapshot_base_directory() {
    let dir = tempfile::tempdir().expect("create temp dir for settings");
    ambient_fs::write(
        dir.path().join("settings.json"),
        r#"{"heartbeat":{"enabled":true,"interval_secs":321}}"#,
    )
    .expect("write snapshot settings.json");
    let context = EnvContext::default().with_env(
        "AXINITE_BASE_DIR",
        dir.path().to_str().expect("UTF-8 temp path"),
    );

    let settings = settings_for_context(&context);

    assert!(settings.heartbeat.enabled);
    assert_eq!(settings.heartbeat.interval_secs, 321);
}

/// A malformed settings file must not abort diagnostics; defaults are used.
#[test]
fn settings_for_context_falls_back_to_defaults_for_invalid_settings() {
    let dir = tempfile::tempdir().expect("create temp dir for settings");
    ambient_fs::write(
        dir.path().join("settings.json"),
        r#"{"onboard_completed": "yes"}"#,
    )
    .expect("write invalid settings.json");
    let context = EnvContext::default().with_env(
        "AXINITE_BASE_DIR",
        dir.path().to_str().expect("UTF-8 temp path"),
    );

    let settings = settings_for_context(&context);

    assert!(!settings.heartbeat.enabled, "defaults should be used");
}

/// Verify that an available binary is reported as a pass.
#[test]
fn check_binary_finds_sh() {
    match check_binary("sh", &["-c", "echo ok"]) {
        CheckResult::Pass(_) => {}
        other => panic!("expected Pass for sh, got: {}", format_result(&other)),
    }
}

/// Verify that a missing binary is reported as a skip rather than a failure.
#[test]
fn check_binary_skips_nonexistent() {
    match check_binary("__axinite_nonexistent_binary__", &["--version"]) {
        CheckResult::Skip(_) => {}
        other => panic!(
            "expected Skip for nonexistent binary, got: {}",
            format_result(&other)
        ),
    }
}

/// Verify that the secrets check skips when no secrets store is configured.
#[test]
fn check_secrets_none_returns_skip() {
    let settings = Settings::default();
    match check_secrets(&settings) {
        CheckResult::Skip(msg) => {
            assert!(
                msg.contains("not configured"),
                "expected 'not configured' in skip message, got: {msg}"
            );
        }
        other => panic!(
            "expected Skip for default settings, got: {}",
            format_result(&other)
        ),
    }
}

/// Verify that the LLM check reports the NearAI backend without a Bedrock model.
#[test]
fn check_llm_config_shows_nearai_model_for_nearai_backend() {
    let settings = Settings::default();
    let ctx = EnvContext::for_testing(Default::default(), Default::default());
    match check_llm_config_with_context(&ctx, &settings) {
        CheckResult::Pass(msg) => {
            assert!(
                msg.contains("backend=nearai"),
                "expected nearai backend, got: {msg}"
            );
            // Must NOT show a bedrock or registry model when backend is nearai
            assert!(
                !msg.contains("anthropic.claude"),
                "should not show bedrock model for nearai backend: {msg}"
            );
        }
        other => panic!(
            "expected Pass for default LLM config, got: {}",
            format_result(&other)
        ),
    }
}

/// Verify that the embeddings check skips when embeddings are disabled by default.
#[test]
fn check_embeddings_disabled_by_default_returns_skip() {
    let settings = Settings::default();
    let ctx = EnvContext::for_testing(Default::default(), Default::default());
    match check_embeddings_with_context(&ctx, &settings) {
        CheckResult::Skip(msg) => {
            assert!(
                msg.contains("disabled"),
                "expected 'disabled' in skip message, got: {msg}"
            );
        }
        other => panic!(
            "expected Skip for disabled embeddings, got: {}",
            format_result(&other)
        ),
    }
}

/// Verify that the routines check passes when routines are enabled by default.
#[test]
fn check_routines_enabled_by_default() {
    let ctx = EnvContext::for_testing(Default::default(), Default::default());
    match check_routines_config_with_context(&ctx) {
        CheckResult::Pass(msg) => {
            assert!(
                msg.contains("enabled"),
                "routines should be enabled by default, got: {msg}"
            );
        }
        other => panic!(
            "expected Pass for default routines, got: {}",
            format_result(&other)
        ),
    }
}

/// Verify that the secrets check fails when an environment-backed key names an unset variable.
#[test]
fn check_secrets_env_without_var_returns_fail() {
    let settings = Settings {
        secrets_master_key_source: crate::settings::KeySource::Env,
        ..Default::default()
    };
    match check_secrets(&settings) {
        CheckResult::Fail(msg) => {
            assert!(
                msg.contains("SECRETS_MASTER_KEY not set"),
                "expected mention of missing env var, got: {msg}"
            );
        }
        CheckResult::Pass(_) => {
            // If SECRETS_MASTER_KEY happens to be set in the environment,
            // Pass is correct — don't fail the test.
        }
        other => panic!(
            "expected Fail or Pass for env key source, got: {}",
            format_result(&other)
        ),
    }
}

fn format_result(r: &CheckResult) -> String {
    match r {
        CheckResult::Pass(s) => format!("Pass({s})"),
        CheckResult::Fail(s) => format!("Fail({s})"),
        CheckResult::Skip(s) => format!("Skip({s})"),
    }
}
