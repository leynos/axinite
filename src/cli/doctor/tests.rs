//! Unit tests for doctor diagnostic checks and result formatting.

use super::CheckResult;
use super::core_checks::{
    check_llm_config, check_llm_config_with_context, check_nearai_session, check_settings_file,
    check_workspace_dir, check_workspace_search,
};
use super::external_checks::{check_binary, check_docker_daemon, check_service_installed};
use super::subsystem_checks::{
    check_embeddings, check_embeddings_with_context, check_gateway_config, check_mcp_config,
    check_routines_config, check_routines_config_with_context, check_secrets, check_skills,
};
use crate::config::EnvContext;
use crate::settings::Settings;

#[test]
fn check_binary_finds_sh() {
    match check_binary("sh", &["-c", "echo ok"]) {
        CheckResult::Pass(_) => {}
        other => panic!("expected Pass for sh, got: {}", format_result(&other)),
    }
}

#[test]
fn check_binary_skips_nonexistent() {
    match check_binary("__ironclaw_nonexistent_binary__", &["--version"]) {
        CheckResult::Skip(_) => {}
        other => panic!(
            "expected Skip for nonexistent binary, got: {}",
            format_result(&other)
        ),
    }
}

#[test]
fn check_workspace_dir_does_not_panic() {
    let result = check_workspace_dir();
    match result {
        CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
    }
}

#[tokio::test]
async fn check_workspace_search_does_not_panic() {
    let result = check_workspace_search().await;
    match result {
        CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
    }
}

#[tokio::test]
async fn check_nearai_session_does_not_panic() {
    let result = check_nearai_session().await;
    match result {
        CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
    }
}

#[test]
fn check_settings_file_handles_missing() {
    // Settings::default_path() might or might not exist, but must not panic
    let result = check_settings_file();
    match result {
        CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
    }
}

#[test]
fn check_llm_config_does_not_panic() {
    let settings = Settings::default();
    let result = check_llm_config(&settings);
    match result {
        CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
    }
}

#[test]
fn check_routines_config_does_not_panic() {
    let result = check_routines_config();
    match result {
        CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
    }
}

#[test]
fn check_gateway_config_does_not_panic() {
    let settings = Settings::default();
    let result = check_gateway_config(&settings);
    match result {
        CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
    }
}

#[test]
fn check_embeddings_does_not_panic() {
    let settings = Settings::default();
    let result = check_embeddings(&settings);
    match result {
        CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
    }
}

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

#[test]
fn check_service_installed_does_not_panic() {
    let result = check_service_installed();
    match result {
        CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
    }
}

#[tokio::test]
async fn check_docker_daemon_does_not_panic() {
    let result = check_docker_daemon().await;
    match result {
        CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
    }
}

#[tokio::test]
async fn check_mcp_config_does_not_panic() {
    let result = check_mcp_config().await;
    match result {
        CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
    }
}

#[tokio::test]
async fn check_skills_does_not_panic() {
    let result = check_skills().await;
    match result {
        CheckResult::Pass(_) | CheckResult::Fail(_) | CheckResult::Skip(_) => {}
    }
}

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
