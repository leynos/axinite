//! Snapshot-selection tests for doctor diagnostics.
//!
//! These live apart from `tests` because the ambient-filesystem fixtures make
//! each case long, and the shared test module has a line budget.

use super::CheckResult;
use super::subsystem_checks::check_embeddings_with_context;
use crate::config::EnvContext;
use crate::settings::Settings;

/// The NEAR AI embeddings credential check must read the session file from the
/// snapshot's base directory, so a snapshot selecting another deployment
/// reports that deployment's credentials rather than the ambient one's.
#[test]
fn check_embeddings_nearai_session_comes_from_the_snapshot() {
    let settings = Settings::default();

    let with_session = tempfile::tempdir().expect("create temp dir with a session");
    ambient_fs::write(with_session.path().join("session.json"), "session-token")
        .expect("write snapshot session file");
    match check_embeddings_with_context(&nearai_context(&with_session), &settings) {
        CheckResult::Pass(msg) => assert!(
            msg.contains("provider=nearai"),
            "expected the snapshot's NEAR AI session to satisfy the check, got: {msg}"
        ),
        _ => panic!("a session inside the snapshot's base directory must pass the check"),
    }

    let without_session = tempfile::tempdir().expect("create temp dir without a session");
    match check_embeddings_with_context(&nearai_context(&without_session), &settings) {
        CheckResult::Fail(msg) => assert!(
            msg.contains("credentials missing"),
            "expected a missing-credentials failure for the empty snapshot, got: {msg}"
        ),
        _ => panic!("an empty snapshot base directory must not inherit an ambient session"),
    }
}

/// An enabled NEAR AI embeddings snapshot rooted at `dir`.
fn nearai_context(dir: &tempfile::TempDir) -> EnvContext {
    EnvContext::default()
        .with_env(
            "AXINITE_BASE_DIR",
            dir.path().to_str().expect("UTF-8 temp path"),
        )
        .with_env("EMBEDDING_ENABLED", "true")
        .with_env("EMBEDDING_PROVIDER", "nearai")
}
