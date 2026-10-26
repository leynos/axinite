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
/// Reads configuration from an immutable [`crate::config::EnvContext`] and
/// checks for the default session file on disk.
#[cfg(any(feature = "postgres", feature = "libsql"))]
pub fn check_onboard_needed() -> Option<&'static str> {
    check_onboard_needed_from(&crate::config::EnvContext::capture_ambient())
}

#[cfg(any(feature = "postgres", feature = "libsql"))]
fn check_onboard_needed_from(ctx: &crate::config::EnvContext) -> Option<&'static str> {
    let has_db = ctx.contains_key("DATABASE_URL")
        || ctx.contains_key("LIBSQL_PATH")
        || crate::config::default_libsql_path_in(&ctx.axinite_base_dir()).exists();

    if !has_db {
        return Some("Database not configured");
    }

    if ctx.get("ONBOARD_COMPLETED") == Some("true") {
        return None;
    }

    if !ctx.contains_key("NEARAI_API_KEY") {
        let session_path = crate::config::llm::default_session_path_in(&ctx.axinite_base_dir());
        if !session_path.exists() {
            return Some("First run");
        }
    }

    None
}

#[cfg(all(test, any(feature = "postgres", feature = "libsql")))]
mod tests {
    //! Tests for onboarding decisions from explicit configuration snapshots.

    use std::collections::HashMap;

    use tempfile::tempdir;

    use super::check_onboard_needed_from;
    use crate::config::EnvContext;

    #[test]
    fn empty_database_and_api_key_values_keep_presence_semantics() {
        let dir = tempdir().expect("create temporary onboarding base directory");
        let context = EnvContext::for_testing(
            HashMap::from([
                (
                    String::from("AXINITE_BASE_DIR"),
                    dir.path().to_string_lossy().into_owned(),
                ),
                (String::from("DATABASE_URL"), String::new()),
                (String::from("NEARAI_API_KEY"), String::new()),
            ]),
            HashMap::new(),
        );

        assert_eq!(check_onboard_needed_from(&context), None);
    }
}
