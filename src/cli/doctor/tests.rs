//! Unit tests for doctor diagnostic checks and result formatting.

use std::cell::RefCell;
use std::rc::Rc;

use super::CheckResult;
use super::check;
use super::core_checks::{
    check_database_with_context, check_llm_config_with_context, check_settings_file,
    check_workspace_search_with_context,
};
use super::doctor_report;
use super::external_checks::check_binary;
use super::settings_for_context;
use super::subsystem_checks::{
    check_embeddings_with_context, check_mcp_config_with_context,
    check_routines_config_with_context, check_secrets_with_context,
};
use crate::cli::report::Report;
use crate::config::EnvContext;
use crate::settings::Settings;

/// Writer that exposes everything written so far without waiting for a flush.
#[derive(Clone, Default)]
struct SharedBuffer(Rc<RefCell<Vec<u8>>>);

impl SharedBuffer {
    fn contents(&self) -> String {
        String::from_utf8_lossy(&self.0.borrow()).into_owned()
    }
}

impl std::io::Write for SharedBuffer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

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
    let ctx = EnvContext::default();
    match check_secrets_with_context(&ctx, &settings) {
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

/// Verify that the secrets check fails when the supplied snapshot lacks an
/// environment-backed master key, even if the ambient process has one.
#[test]
fn check_secrets_env_without_var_returns_fail() {
    let settings = Settings {
        secrets_master_key_source: crate::settings::KeySource::Env,
        ..Default::default()
    };
    let ctx = EnvContext::for_testing(Default::default(), Default::default());
    match check_secrets_with_context(&ctx, &settings) {
        CheckResult::Fail(msg) => {
            assert!(
                msg.contains("SECRETS_MASTER_KEY not set"),
                "expected mention of missing env var, got: {msg}"
            );
        }
        other => panic!(
            "expected Fail for env key source without the variable, got: {}",
            format_result(&other)
        ),
    }
}

/// The secrets check must read the master key from the supplied snapshot.
///
/// A key present only in the snapshot proves the check no longer reads the
/// ambient process environment.
#[test]
fn check_secrets_env_with_snapshot_var_returns_pass() {
    let settings = Settings {
        secrets_master_key_source: crate::settings::KeySource::Env,
        ..Default::default()
    };
    let ctx = EnvContext::default().with_env("SECRETS_MASTER_KEY", "from-snapshot");
    match check_secrets_with_context(&ctx, &settings) {
        CheckResult::Pass(msg) => {
            assert!(
                msg.contains("env var (set)"),
                "expected env-sourced pass, got: {msg}"
            );
        }
        other => panic!(
            "expected Pass for a snapshot-backed key, got: {}",
            format_result(&other)
        ),
    }
}

/// The MCP check must load the configuration from the snapshot's base
/// directory rather than the ambient deployment.
///
/// The snapshot points at a temporary directory whose config file is invalid,
/// so observing a failure proves the check read that file.
#[tokio::test]
async fn check_mcp_config_reads_the_snapshot_base_directory() {
    let dir = tempfile::tempdir().expect("create temp dir for MCP config");
    ambient_fs::write(
        dir.path().join("mcp-servers.json"),
        r#"{"servers":[{"name":"","url":"https://example.com"}]}"#,
    )
    .expect("write invalid mcp-servers.json");
    let ctx = EnvContext::default().with_env(
        "AXINITE_BASE_DIR",
        dir.path().to_str().expect("UTF-8 temp path"),
    );

    match check_mcp_config_with_context(&ctx).await {
        CheckResult::Fail(msg) => assert!(
            msg.contains("invalid"),
            "expected an invalid-server failure, got: {msg}"
        ),
        other => panic!(
            "expected Fail for the snapshot's invalid MCP config, got: {}",
            format_result(&other)
        ),
    }
}

/// Diagnostics must reach the writer as each check completes, not after the
/// whole report is assembled.
#[test]
fn diagnostics_stream_before_later_checks_run() {
    let buffer = SharedBuffer::default();
    let observed = buffer.clone();
    let mut report = Report::new(buffer);
    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut skipped = 0u32;

    check(
        "First",
        CheckResult::Pass("ok".into()),
        &mut passed,
        &mut failed,
        &mut skipped,
        &mut report,
    );
    assert!(
        observed.contents().contains("[pass] First: ok"),
        "the first diagnostic must be visible before the next check runs; saw: {}",
        observed.contents()
    );

    check(
        "Second",
        CheckResult::Skip("n/a".into()),
        &mut passed,
        &mut failed,
        &mut skipped,
        &mut report,
    );
    let text = observed.contents();
    assert!(
        text.contains("[pass] First: ok") && text.contains("[skip] Second: n/a"),
        "both diagnostics must be present once their checks complete; saw:\n{text}"
    );
}

fn format_result(r: &CheckResult) -> String {
    match r {
        CheckResult::Pass(s) => format!("Pass({s})"),
        CheckResult::Fail(s) => format!("Fail({s})"),
        CheckResult::Skip(s) => format!("Skip({s})"),
    }
}

/// The doctor command boundary must inspect the deployment selected by the
/// supplied snapshot, so a divergent `AXINITE_BASE_DIR` changes the report.
#[tokio::test]
async fn doctor_report_uses_the_snapshot_base_directory() {
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

    let report = doctor_report(&context).await;

    assert!(
        report.contains("Axinite Doctor"),
        "report must render the doctor header, got:\n{report}"
    );
    assert!(
        report.contains("[FAIL] Settings file: settings.json is invalid"),
        "report must fail the snapshot's invalid settings file, got:\n{report}"
    );
    assert!(
        report.contains(&dir.path().join("settings.json").display().to_string()),
        "report must name the snapshot's settings path, got:\n{report}"
    );
}
