//! Bootstrap helpers for Axinite.
//!
//! Bootstrap settings needed before a database connection are persisted in
//! `.env`. `DotenvLayers` parses those files into `EnvContext`; values are not
//! copied into the process environment.
//!
//! Default file: `~/.axinite/.env` (standard dotenvy format)

mod migration;
mod pid_lock;
pub mod tools;

pub(crate) use migration::migrate_bootstrap_json_to_env;
pub use migration::{MigrationError, migrate_disk_to_db};
pub use pid_lock::{PidLock, PidLockError, pid_lock_path};

use std::path::PathBuf;
use std::sync::LazyLock;

use crate::config::EnvContext;

const AXINITE_BASE_DIR_ENV: &str = "AXINITE_BASE_DIR";

/// Lazily computed Axinite base directory, cached for the lifetime of the process.
static AXINITE_BASE_DIR: LazyLock<PathBuf> = LazyLock::new(compute_axinite_base_dir);

/// Compute the Axinite base directory from environment.
///
/// This is the underlying implementation used by both the public
/// `axinite_base_dir()` function (which caches the result) and tests
/// (which need to verify different configurations).
pub fn compute_axinite_base_dir() -> PathBuf {
    compute_axinite_base_dir_from(&EnvContext::capture_ambient())
}

/// Compute the Axinite base directory from an explicit environment snapshot.
pub fn compute_axinite_base_dir_from(ctx: &EnvContext) -> PathBuf {
    if let Some(path) = relative_base_dir(ctx) {
        eprintln!(
            "Warning: AXINITE_BASE_DIR is a relative path '{}', resolved against current directory",
            path.display()
        );
    }
    ctx.axinite_base_dir()
}

fn relative_base_dir(ctx: &EnvContext) -> Option<PathBuf> {
    ctx.get(AXINITE_BASE_DIR_ENV)
        .map(PathBuf::from)
        .filter(|path| !path.is_absolute())
}

/// Get the Axinite base directory.
///
/// Override with `AXINITE_BASE_DIR` from the process environment or the
/// current-directory `.env` layer. The bootstrap `.env` cannot relocate itself.
/// Defaults to `~/.axinite` (or `./.axinite` if home directory cannot be determined).
///
/// The dotenv snapshot is refreshed before the first access; the selected root
/// is then cached for the lifetime of the process.
///
/// # Environment Variable Behaviour
/// - If `AXINITE_BASE_DIR` is set to a non-empty path, that path is used.
/// - If `AXINITE_BASE_DIR` is set to an empty string, it is treated as unset.
/// - Relative paths are resolved against the captured current directory.
/// - If no home directory is available, the current directory is used.
///
/// # Returns
/// A `PathBuf` pointing to the base directory. The path is not validated
/// for existence.
pub fn axinite_base_dir() -> PathBuf {
    if LazyLock::get(&AXINITE_BASE_DIR).is_none() {
        EnvContext::refresh_dotenv_layers();
    }
    AXINITE_BASE_DIR.clone()
}

/// Path to the Axinite-specific `.env` file: `~/.axinite/.env`.
pub fn axinite_env_path() -> PathBuf {
    axinite_base_dir().join(".env")
}

/// Return the already cached root without causing its lazy initialization.
pub(crate) fn cached_axinite_base_dir() -> Option<PathBuf> {
    LazyLock::get(&AXINITE_BASE_DIR).cloned()
}

/// Write database bootstrap vars to `~/.axinite/.env`.
///
/// These settings form the chicken-and-egg layer: they must be available from
/// the filesystem before any database connection because they determine which
/// database to connect to. `EnvContext` reads them without exporting them to
/// the process environment. Everything else is stored in the database itself.
///
/// Creates the parent directory if it doesn't exist.
/// Values are double-quoted so that `#` (common in URL-encoded passwords)
/// and other shell-special characters are preserved by the dotenv parser.
pub fn save_bootstrap_env(vars: &[(&str, &str)]) -> std::io::Result<()> {
    save_bootstrap_env_to(&axinite_env_path(), vars)
}

/// Write bootstrap vars to an arbitrary path (testable variant).
///
/// Values are double-quoted and escaped so that `#`, `"`, `\` and other
/// shell-special characters are preserved by the dotenv parser.
pub fn save_bootstrap_env_to(path: &std::path::Path, vars: &[(&str, &str)]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ambient_fs::create_dir_all(parent)?;
    }
    let mut content = String::new();
    for (key, value) in vars {
        // Escape backslashes and double quotes to prevent env var injection
        // (e.g. a value containing `"\nINJECTED="x` would break out of quotes).
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        content.push_str(&format!("{}=\"{}\"\n", key, escaped));
    }
    ambient_fs::write(path, &content)?;
    restrict_file_permissions(path)?;
    Ok(())
}

/// Update or add multiple variables in `~/.axinite/.env`, preserving existing content.
///
/// Like `upsert_bootstrap_var` but batched — replaces lines for any key in `vars`
/// and preserves all other existing lines. Use this instead of `save_bootstrap_env`
/// when you want to update specific keys without destroying user-added variables.
pub fn upsert_bootstrap_vars(vars: &[(&str, &str)]) -> std::io::Result<()> {
    upsert_bootstrap_vars_to(&axinite_env_path(), vars)
}

/// Update or add multiple variables at an arbitrary path (testable variant).
pub fn upsert_bootstrap_vars_to(
    path: &std::path::Path,
    vars: &[(&str, &str)],
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ambient_fs::create_dir_all(parent)?;
    }

    let keys_being_written: std::collections::HashSet<&str> =
        vars.iter().map(|(k, _)| *k).collect();

    let existing = match ambient_fs::read_to_string(path) {
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

    ambient_fs::write(path, &result)?;
    restrict_file_permissions(path)?;
    Ok(())
}

/// Update or add a single variable in `~/.axinite/.env`, preserving existing content.
///
/// Unlike `save_bootstrap_env` (which overwrites the entire file), this
/// reads the current `.env`, replaces the line for `key` if it exists,
/// or appends it otherwise. Use this when writing a single bootstrap var
/// outside the wizard (which manages the full set via `save_bootstrap_env`).
pub fn upsert_bootstrap_var(key: &str, value: &str) -> std::io::Result<()> {
    upsert_bootstrap_var_to(&axinite_env_path(), key, value)
}

/// Update or add a single variable at an arbitrary path (testable variant).
pub fn upsert_bootstrap_var_to(
    path: &std::path::Path,
    key: &str,
    value: &str,
) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ambient_fs::create_dir_all(parent)?;
    }

    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    let new_line = format!("{}=\"{}\"", key, escaped);
    let prefix = format!("{}=", key);

    let existing = ambient_fs::read_to_string(path).unwrap_or_default();

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

    ambient_fs::write(path, result)?;
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
        let perms = ambient_fs::Permissions::from_mode(0o600);
        ambient_fs::set_permissions(_path, perms)?;
    }
    Ok(())
}

/// Write `DATABASE_URL` to `~/.axinite/.env`.
///
/// Convenience wrapper around `save_bootstrap_env` for single-value migration
/// paths. Prefer `save_bootstrap_env` for new code.
pub fn save_database_url(url: &str) -> std::io::Result<()> {
    save_bootstrap_env(&[("DATABASE_URL", url)])
}

#[cfg(test)]
mod tests;
