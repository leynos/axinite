//! Interactive prompts used by the database setup step (Step 1).

use super::*;

/// Prompt for optional Turso cloud sync credentials.
///
/// Returns `(None, None)` when sync is declined or when the entered
/// credentials are incomplete.
pub(super) fn prompt_turso_sync() -> Result<(Option<String>, Option<String>), SetupError> {
    let use_turso =
        confirm("Enable Turso cloud sync (remote replica)?", false).map_err(SetupError::Io)?;
    if !use_turso {
        return Ok((None, None));
    }

    print_info("Enter your Turso database URL and auth token.");
    print_info("Format: libsql://your-db.turso.io");
    println!();

    let url = input("Turso URL").map_err(SetupError::Io)?;
    if url.is_empty() {
        print_error("Turso URL is required for cloud sync.");
        return Ok((None, None));
    }

    let token_secret = secret_input("Auth token").map_err(SetupError::Io)?;
    let token = token_secret.expose_secret().to_string();
    if token.is_empty() {
        print_error("Auth token is required for cloud sync.");
        return Ok((None, None));
    }

    Ok((Some(url), Some(token)))
}
