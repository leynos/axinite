//! Interactive setup wizard for Axinite.
//!
//! Provides a guided setup experience for:
//! 1. Database connection
//! 2. Security (secrets master key)
//! 3. Inference provider selection
//! 4. Model selection
//! 5. Embeddings
//! 6. Channel configuration (HTTP, Telegram, etc.)
//! 7. Extensions (tool installation from registry)
//! 8. Heartbeat (background tasks)
//!
//! # Example
//!
//! ```ignore
//! use axinite::setup::SetupWizard;
//!
//! let mut wizard = SetupWizard::new();
//! wizard.run().await?;
//! ```

mod channels;
mod nearai;
#[cfg(any(feature = "postgres", feature = "libsql"))]
mod persistence;
mod prompts;
#[cfg(any(feature = "postgres", feature = "libsql"))]
mod wizard;

pub use channels::{
    ChannelSetupError, SecretsContext, SignalSetupResult, WasmChannelSetupResult, setup_http,
    setup_signal, setup_tunnel, setup_wasm_channel,
};
pub use prompts::{
    confirm, input, optional_input, print_error, print_header, print_info, print_step,
    print_success, secret_input, select_many, select_one,
};
#[cfg(any(feature = "postgres", feature = "libsql"))]
pub use wizard::{SetupConfig, SetupWizard};

/// Check if onboarding is needed and return the reason.
///
/// Reads `DATABASE_URL`, `LIBSQL_PATH`, `ONBOARD_COMPLETED`, and
/// `NEARAI_API_KEY` from the supplied snapshot, and checks for the default
/// libSQL database and session files under that snapshot's base directory.
#[cfg(any(feature = "postgres", feature = "libsql"))]
pub fn check_onboard_needed(env_context: &crate::config::EnvContext) -> Option<&'static str> {
    let base_dir = env_context.axinite_base_dir();
    let has_db = env_context.get("DATABASE_URL").is_some()
        || env_context.get("LIBSQL_PATH").is_some()
        || crate::config::default_libsql_path_in(&base_dir).exists();

    if !has_db {
        return Some("Database not configured");
    }

    if env_context.get("ONBOARD_COMPLETED") == Some("true") {
        return None;
    }

    if env_context.get("NEARAI_API_KEY").is_none() && !base_dir.join("session.json").exists() {
        return Some("First run");
    }

    None
}

#[cfg(all(test, any(feature = "postgres", feature = "libsql")))]
mod tests {
    //! Onboarding-detection tests. Detection reads only the supplied snapshot.

    use super::check_onboard_needed;
    use crate::config::EnvContext;

    /// A snapshot base directory in a fresh temporary directory.
    fn snapshot(dir: &tempfile::TempDir) -> EnvContext {
        EnvContext::default().with_env("AXINITE_BASE_DIR", dir.path().display().to_string())
    }

    /// A database configured in the snapshot satisfies the check even though
    /// the ambient environment holds no database configuration.
    #[test]
    fn check_onboard_needed_reads_the_snapshot_database() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = snapshot(&dir)
            .with_env("DATABASE_URL", "postgres://example/db")
            .with_env("ONBOARD_COMPLETED", "true");

        assert_eq!(check_onboard_needed(&ctx), None);
    }

    /// Without a database in the snapshot, onboarding is still required.
    #[test]
    fn check_onboard_needed_reports_missing_database_from_the_snapshot() {
        let dir = tempfile::tempdir().expect("tempdir");

        assert_eq!(
            check_onboard_needed(&snapshot(&dir)),
            Some("Database not configured")
        );
    }

    /// A libSQL database file inside the snapshot's base directory satisfies
    /// the database check without any environment variable.
    #[test]
    fn check_onboard_needed_detects_a_libsql_file_in_the_snapshot_base_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        ambient_fs::write(dir.path().join("axinite.db"), "").expect("write database file");
        let ctx = snapshot(&dir).with_env("ONBOARD_COMPLETED", "true");

        assert_eq!(check_onboard_needed(&ctx), None);
    }

    /// With a database configured but onboarding unfinished, the reason names
    /// the first run.
    #[test]
    fn check_onboard_needed_reports_first_run_from_the_snapshot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = snapshot(&dir).with_env("DATABASE_URL", "postgres://example/db");

        assert_eq!(check_onboard_needed(&ctx), Some("First run"));
    }

    /// A session file in the snapshot's base directory satisfies the first-run
    /// check without `NEARAI_API_KEY`.
    #[test]
    fn check_onboard_needed_accepts_a_snapshot_session_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        ambient_fs::write(dir.path().join("session.json"), "{}").expect("write session file");
        let ctx = snapshot(&dir).with_env("DATABASE_URL", "postgres://example/db");

        assert_eq!(check_onboard_needed(&ctx), None);
    }
}
