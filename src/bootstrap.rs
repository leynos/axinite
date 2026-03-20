//! Bootstrap helpers for IronClaw.
//!
//! The only setting that truly needs disk persistence before the database is
//! available is `DATABASE_URL` (chicken-and-egg: can't connect to DB without
//! it). Everything else is auto-detected or read from env vars.
//!
//! File: `~/.ironclaw/.env` (standard dotenvy format)

mod migration;
mod pid_lock;
pub mod tools;

pub use migration::{MigrationError, migrate_disk_to_db};
pub use pid_lock::{PidLock, PidLockError, pid_lock_path};

use std::path::PathBuf;
use std::sync::LazyLock;

use migration::migrate_bootstrap_json_to_env;

const IRONCLAW_BASE_DIR_ENV: &str = "IRONCLAW_BASE_DIR";

/// Lazily computed IronClaw base directory, cached for the lifetime of the process.
static IRONCLAW_BASE_DIR: LazyLock<PathBuf> = LazyLock::new(compute_ironclaw_base_dir);

/// Compute the IronClaw base directory from environment.
///
/// This is the underlying implementation used by both the public
/// `ironclaw_base_dir()` function (which caches the result) and tests
/// (which need to verify different configurations).
pub fn compute_ironclaw_base_dir() -> PathBuf {
    std::env::var(IRONCLAW_BASE_DIR_ENV)
        .map(PathBuf::from)
        .map(|path| {
            if path.as_os_str().is_empty() {
                default_base_dir()
            } else if !path.is_absolute() {
                eprintln!(
                    "Warning: IRONCLAW_BASE_DIR is a relative path '{}', resolved against current directory",
                    path.display()
                );
                path
            } else {
                path
            }
        })
        .unwrap_or_else(|_| default_base_dir())
}

/// Get the default IronClaw base directory (~/.ironclaw).
///
/// Logs a warning if the home directory cannot be determined and falls back to
/// the current directory.
fn default_base_dir() -> PathBuf {
    if let Some(home) = dirs::home_dir() {
        home.join(".ironclaw")
    } else {
        eprintln!("Warning: Could not determine home directory, using current directory");
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/tmp"))
            .join(".ironclaw")
    }
}

/// Get the IronClaw base directory.
///
/// Override with `IRONCLAW_BASE_DIR` environment variable.
/// Defaults to `~/.ironclaw` (or `./.ironclaw` if home directory cannot be determined).
///
/// Thread-safe: the value is computed once and cached in a `LazyLock`.
///
/// # Environment Variable Behavior
/// - If `IRONCLAW_BASE_DIR` is set to a non-empty path, that path is used.
/// - If `IRONCLAW_BASE_DIR` is set to an empty string, it is treated as unset.
/// - If `IRONCLAW_BASE_DIR` contains null bytes, a warning is printed and the default is used.
/// - If the home directory cannot be determined, a warning is printed and the current directory is used.
///
/// # Returns
/// A `PathBuf` pointing to the base directory. The path is not validated
/// for existence.
pub fn ironclaw_base_dir() -> PathBuf {
    IRONCLAW_BASE_DIR.clone()
}

/// Path to the IronClaw-specific `.env` file: `~/.ironclaw/.env`.
pub fn ironclaw_env_path() -> PathBuf {
    ironclaw_base_dir().join(".env")
}

/// Load env vars from `~/.ironclaw/.env` (in addition to the standard `.env`).
///
/// Call this **after** `dotenvy::dotenv()` so that the standard `./.env`
/// takes priority over `~/.ironclaw/.env`. dotenvy never overwrites
/// existing env vars, so the effective priority is:
///
///   explicit env vars > `./.env` > `~/.ironclaw/.env` > auto-detect
///
/// If `~/.ironclaw/.env` doesn't exist but the legacy `bootstrap.json` does,
/// extracts `DATABASE_URL` from it and writes the `.env` file (one-time
/// upgrade from the old config format).
///
/// After loading the `.env` file, auto-detects the libsql backend: if
/// `DATABASE_BACKEND` is still unset and `~/.ironclaw/ironclaw.db` exists,
/// defaults to `libsql` so cloud instances work out of the box without any
/// manual configuration.
pub fn load_ironclaw_env() {
    let path = ironclaw_env_path();

    if !path.exists() {
        // One-time upgrade: extract DATABASE_URL from legacy bootstrap.json
        migrate_bootstrap_json_to_env(&path);
    }

    if path.exists() {
        let _ = dotenvy::from_path(&path);
    }

    // Auto-detect libsql: if DATABASE_BACKEND is still unset after loading
    // all env files, and the local SQLite DB exists, default to libsql.
    // This avoids the chicken-and-egg problem on cloud instances where no
    // DATABASE_URL is configured but ironclaw.db is already present.
    if std::env::var("DATABASE_BACKEND").is_err() {
        let default_db = dirs::home_dir()
            .unwrap_or_default()
            .join(".ironclaw")
            .join("ironclaw.db");
        if default_db.exists() {
            // SAFETY: `load_ironclaw_env` is called from a synchronous `fn main()`
            // before the Tokio runtime is started, so no other threads exist yet.
            unsafe { std::env::set_var("DATABASE_BACKEND", "libsql") };
        }
    }
}

/// Write database bootstrap vars to `~/.ironclaw/.env`.
///
/// These settings form the chicken-and-egg layer: they must be available
/// from the filesystem (env vars) BEFORE any database connection, because
/// they determine which database to connect to. Everything else is stored
/// in the database itself.
///
/// Creates the parent directory if it doesn't exist.
/// Values are double-quoted so that `#` (common in URL-encoded passwords)
/// and other shell-special characters are preserved by dotenvy.
pub fn save_bootstrap_env(vars: &[(&str, &str)]) -> std::io::Result<()> {
    save_bootstrap_env_to(&ironclaw_env_path(), vars)
}

/// Write bootstrap vars to an arbitrary path (testable variant).
///
/// Values are double-quoted and escaped so that `#`, `"`, `\` and other
/// shell-special characters are preserved by dotenvy.
pub fn save_bootstrap_env_to(path: &std::path::Path, vars: &[(&str, &str)]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = String::new();
    for (key, value) in vars {
        // Escape backslashes and double quotes to prevent env var injection
        // (e.g. a value containing `"\nINJECTED="x` would break out of quotes).
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        content.push_str(&format!("{}=\"{}\"\n", key, escaped));
    }
    std::fs::write(path, &content)?;
    restrict_file_permissions(path)?;
    Ok(())
}

/// Update or add multiple variables in `~/.ironclaw/.env`, preserving existing content.
///
/// Like `upsert_bootstrap_var` but batched — replaces lines for any key in `vars`
/// and preserves all other existing lines. Use this instead of `save_bootstrap_env`
/// when you want to update specific keys without destroying user-added variables.
pub fn upsert_bootstrap_vars(vars: &[(&str, &str)]) -> std::io::Result<()> {
    upsert_bootstrap_vars_to(&ironclaw_env_path(), vars)
}

/// Update or add multiple variables at an arbitrary path (testable variant).
pub fn upsert_bootstrap_vars_to(
    path: &std::path::Path,
    vars: &[(&str, &str)],
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let keys_being_written: std::collections::HashSet<&str> =
        vars.iter().map(|(k, _)| *k).collect();

    let existing = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };

    let mut result = String::new();
    for line in existing.lines() {
        // Extract key from lines matching `KEY=...`
        let is_overwritten = line
            .split_once('=')
            .map(|(k, _)| keys_being_written.contains(k.trim()))
            .unwrap_or(false);

        if !is_overwritten {
            result.push_str(line);
            result.push('\n');
        }
    }

    // Append all new key=value pairs
    for (key, value) in vars {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        result.push_str(&format!("{}=\"{}\"\n", key, escaped));
    }

    std::fs::write(path, &result)?;
    restrict_file_permissions(path)?;
    Ok(())
}

/// Update or add a single variable in `~/.ironclaw/.env`, preserving existing content.
///
/// Unlike `save_bootstrap_env` (which overwrites the entire file), this
/// reads the current `.env`, replaces the line for `key` if it exists,
/// or appends it otherwise. Use this when writing a single bootstrap var
/// outside the wizard (which manages the full set via `save_bootstrap_env`).
pub fn upsert_bootstrap_var(key: &str, value: &str) -> std::io::Result<()> {
    upsert_bootstrap_var_to(&ironclaw_env_path(), key, value)
}

/// Update or add a single variable at an arbitrary path (testable variant).
pub fn upsert_bootstrap_var_to(
    path: &std::path::Path,
    key: &str,
    value: &str,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    let new_line = format!("{}=\"{}\"", key, escaped);
    let prefix = format!("{}=", key);

    let existing = std::fs::read_to_string(path).unwrap_or_default();

    let mut found = false;
    let mut result = String::new();
    for line in existing.lines() {
        if line.starts_with(&prefix) {
            if !found {
                result.push_str(&new_line);
                result.push('\n');
                found = true;
            }
            // Skip duplicate lines for this key
            continue;
        }
        result.push_str(line);
        result.push('\n');
    }

    if !found {
        result.push_str(&new_line);
        result.push('\n');
    }

    std::fs::write(path, result)?;
    restrict_file_permissions(path)?;
    Ok(())
}

/// Set restrictive file permissions (0o600) on Unix systems.
///
/// The `.env` file may contain database credentials and API keys,
/// so it should only be readable by the owner.
fn restrict_file_permissions(_path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(_path, perms)?;
    }
    Ok(())
}

/// Write `DATABASE_URL` to `~/.ironclaw/.env`.
///
/// Convenience wrapper around `save_bootstrap_env` for single-value migration
/// paths. Prefer `save_bootstrap_env` for new code.
pub fn save_database_url(url: &str) -> std::io::Result<()> {
    save_bootstrap_env(&[("DATABASE_URL", url)])
}

#[cfg(test)]
mod tests;
