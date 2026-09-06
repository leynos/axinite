//! Unit tests for doctor diagnostic checks and result formatting.

use super::CheckResult;
use super::core_checks::check_llm_config_with_context;
use super::external_checks::check_binary;
use super::subsystem_checks::{
    check_embeddings_with_context, check_routines_config_with_context, check_secrets,
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
    match check_binary("__axinite_nonexistent_binary__", &["--version"]) {
        CheckResult::Skip(_) => {}
        other => panic!(
            "expected Skip for nonexistent binary, got: {}",
            format_result(&other)
        ),
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
