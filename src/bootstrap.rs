//! Bootstrap helpers for IronClaw.
//!
//! The only setting that truly needs disk persistence before the database is
//! available is `DATABASE_URL` (chicken-and-egg: can't connect to DB without
//! it). Everything else is auto-detected or read from env vars.
//!
//! File: `~/.ironclaw/.env` (standard dotenvy format)

pub mod tools;

use std::path::PathBuf;
use std::sync::LazyLock;

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

/// If `bootstrap.json` exists, pull `database_url` out of it and write `.env`.
fn migrate_bootstrap_json_to_env(env_path: &std::path::Path) {
    let ironclaw_dir = env_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let bootstrap_path = ironclaw_dir.join("bootstrap.json");

    if !bootstrap_path.exists() {
        return;
    }

    let content = match std::fs::read_to_string(&bootstrap_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    // Minimal parse: just grab database_url from the JSON
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return,
    };

    if let Some(url) = parsed.get("database_url").and_then(|v| v.as_str()) {
        if let Some(parent) = env_path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!("Warning: failed to create {}: {}", parent.display(), e);
            return;
        }
        if let Err(e) = std::fs::write(env_path, format!("DATABASE_URL=\"{}\"\n", url)) {
            eprintln!("Warning: failed to migrate bootstrap.json to .env: {}", e);
            return;
        }
        rename_to_migrated(&bootstrap_path);
        eprintln!(
            "Migrated DATABASE_URL from bootstrap.json to {}",
            env_path.display()
        );
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

/// One-time migration of legacy `~/.ironclaw/settings.json` into the database.
///
/// Only runs when a `settings.json` exists on disk AND the DB has no settings
/// yet. After the wizard writes directly to the DB, this path is only hit by
/// users upgrading from the old disk-only configuration.
///
/// After syncing, renames `settings.json` to `.migrated` so it won't trigger again.
pub async fn migrate_disk_to_db(
    store: &dyn crate::db::Database,
    user_id: &str,
) -> Result<(), MigrationError> {
    let ironclaw_dir = ironclaw_base_dir();
    let legacy_settings_path = ironclaw_dir.join("settings.json");

    if !legacy_settings_path.exists() {
        tracing::debug!("No legacy settings.json found, skipping disk-to-DB migration");
        return Ok(());
    }

    // If DB already has settings, this is not a first boot, the wizard already
    // wrote directly to the DB. Just clean up the stale file.
    let has_settings = store.has_settings(user_id).await.map_err(|e| {
        MigrationError::Database(format!("Failed to check existing settings: {}", e))
    })?;
    if has_settings {
        tracing::info!("DB already has settings, renaming stale settings.json");
        rename_to_migrated(&legacy_settings_path);
        return Ok(());
    }

    tracing::info!("Migrating disk settings to database...");

    // 1. Load and migrate settings.json
    let settings = crate::settings::Settings::load_from(&legacy_settings_path);
    let db_map = settings.to_db_map();
    if !db_map.is_empty() {
        store
            .set_all_settings(user_id, &db_map)
            .await
            .map_err(|e| {
                MigrationError::Database(format!("Failed to write settings to DB: {}", e))
            })?;
        tracing::info!("Migrated {} settings to database", db_map.len());
    }

    // 2. Write DATABASE_URL to ~/.ironclaw/.env
    if let Some(ref url) = settings.database_url {
        save_database_url(url)
            .map_err(|e| MigrationError::Io(format!("Failed to write .env: {}", e)))?;
        tracing::info!("Wrote DATABASE_URL to {}", ironclaw_env_path().display());
    }

    // 3. Migrate mcp-servers.json if it exists
    let mcp_path = ironclaw_dir.join("mcp-servers.json");
    if mcp_path.exists() {
        match std::fs::read_to_string(&mcp_path) {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(value) => {
                    store
                        .set_setting(user_id, "mcp_servers", &value)
                        .await
                        .map_err(|e| {
                            MigrationError::Database(format!(
                                "Failed to write MCP servers to DB: {}",
                                e
                            ))
                        })?;
                    tracing::info!("Migrated mcp-servers.json to database");

                    rename_to_migrated(&mcp_path);
                }
                Err(e) => {
                    tracing::warn!("Failed to parse mcp-servers.json: {}", e);
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read mcp-servers.json: {}", e);
            }
        }
    }

    // 4. Migrate session.json if it exists
    let session_path = ironclaw_dir.join("session.json");
    if session_path.exists() {
        match std::fs::read_to_string(&session_path) {
            Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(value) => {
                    store
                        .set_setting(user_id, "nearai.session_token", &value)
                        .await
                        .map_err(|e| {
                            MigrationError::Database(format!(
                                "Failed to write session to DB: {}",
                                e
                            ))
                        })?;
                    tracing::info!("Migrated session.json to database");

                    rename_to_migrated(&session_path);
                }
                Err(e) => {
                    tracing::warn!("Failed to parse session.json: {}", e);
                }
            },
            Err(e) => {
                tracing::warn!("Failed to read session.json: {}", e);
            }
        }
    }

    // 5. Rename settings.json to .migrated (don't delete, safety net)
    rename_to_migrated(&legacy_settings_path);

    // 6. Clean up old bootstrap.json if it exists (superseded by .env)
    let old_bootstrap = ironclaw_dir.join("bootstrap.json");
    if old_bootstrap.exists() {
        rename_to_migrated(&old_bootstrap);
        tracing::info!("Renamed old bootstrap.json to .migrated");
    }

    tracing::info!("Disk-to-DB migration complete");
    Ok(())
}

/// Rename a file to `<name>.migrated` as a safety net.
fn rename_to_migrated(path: &std::path::Path) {
    let mut migrated = path.as_os_str().to_owned();
    migrated.push(".migrated");
    if let Err(e) = std::fs::rename(path, &migrated) {
        tracing::warn!("Failed to rename {} to .migrated: {}", path.display(), e);
    }
}

/// Errors that can occur during disk-to-DB migration.
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("IO error: {0}")]
    Io(String),
}

// ── PID Lock ──────────────────────────────────────────────────────────────

/// Path to the PID lock file: `~/.ironclaw/ironclaw.pid`.
pub fn pid_lock_path() -> PathBuf {
    ironclaw_base_dir().join("ironclaw.pid")
}

/// A PID-based lock that prevents multiple IronClaw instances from running
/// simultaneously.
///
/// Uses `fs4::try_lock_exclusive()` for atomic locking (no TOCTOU race),
/// then writes the current PID into the locked file for diagnostics.
/// The OS-level lock is held for the lifetime of this struct and
/// automatically released on drop (along with the PID file cleanup).
#[derive(Debug)]
pub struct PidLock {
    path: PathBuf,
    /// Held open to maintain the OS-level exclusive lock.
    _file: std::fs::File,
}

/// Errors from PID lock acquisition.
#[derive(Debug, thiserror::Error)]
pub enum PidLockError {
    #[error("Another IronClaw instance is already running (PID {pid})")]
    AlreadyRunning { pid: u32 },
    #[error("Failed to acquire PID lock: {0}")]
    Io(#[from] std::io::Error),
}

impl PidLock {
    /// Try to acquire the PID lock.
    ///
    /// Uses an exclusive file lock (`flock`/`LockFileEx`) so that two
    /// concurrent processes cannot both acquire the lock — no TOCTOU race.
    /// If the lock file exists but the holding process is gone (stale),
    /// the lock is reclaimed automatically by the OS.
    pub fn acquire() -> Result<Self, PidLockError> {
        Self::acquire_at(pid_lock_path())
    }

    /// Acquire at a specific path (for testing).
    fn acquire_at(path: PathBuf) -> Result<Self, PidLockError> {
        use fs4::FileExt;
        use std::fs::OpenOptions;
        use std::io::Write;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Open (or create) the lock file
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;

        // Try non-blocking exclusive lock — if another process holds it,
        // this fails immediately instead of blocking.
        if let Err(e) = file.try_lock_exclusive() {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                // Lock held by another process — read its PID for the error message
                let pid = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok())
                    .unwrap_or(0);
                return Err(PidLockError::AlreadyRunning { pid });
            }
            // Other errors (permissions, unsupported filesystem, etc.)
            return Err(PidLockError::Io(e));
        }

        // We hold the exclusive lock — write our PID
        file.set_len(0)?; // truncate
        write!(file, "{}", std::process::id())?;

        Ok(PidLock { path, _file: file })
    }
}

impl Drop for PidLock {
    fn drop(&mut self) {
        // Remove the PID file; the OS-level lock is released when _file is dropped.
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests;
