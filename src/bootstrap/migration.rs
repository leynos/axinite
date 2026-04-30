//! Legacy bootstrap and disk-to-database migration helpers.

use std::{borrow::Cow, io, path::Path};

use crate::bootstrap::{ironclaw_base_dir, ironclaw_env_path, save_database_url};
use crate::db::{SettingKey, UserId};

#[derive(Debug)]
struct SidecarSpec<'a> {
    user_id: &'a str,
    path: &'a std::path::Path,
    file_name: &'a str,
    setting_key: &'a str,
    db_error_msg: &'a str,
    success_msg: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KnownEnvKey {
    DatabaseUrl,
    DatabaseBackend,
    LlmBackend,
    OnboardCompleted,
    EmbeddingEnabled,
}

impl KnownEnvKey {
    const fn as_str(self) -> &'static str {
        match self {
            Self::DatabaseUrl => "DATABASE_URL",
            Self::DatabaseBackend => "DATABASE_BACKEND",
            Self::LlmBackend => "LLM_BACKEND",
            Self::OnboardCompleted => "ONBOARD_COMPLETED",
            Self::EmbeddingEnabled => "EMBEDDING_ENABLED",
        }
    }
}

struct EnvPair<'a> {
    key: KnownEnvKey,
    value: Cow<'a, str>,
}

const _: [KnownEnvKey; 5] = [
    KnownEnvKey::DatabaseUrl,
    KnownEnvKey::DatabaseBackend,
    KnownEnvKey::LlmBackend,
    KnownEnvKey::OnboardCompleted,
    KnownEnvKey::EmbeddingEnabled,
];

/// If `bootstrap.json` exists, pull `database_url` out of it and write `.env`.
pub(crate) fn migrate_bootstrap_json_to_env(env_path: &Path) {
    let ironclaw_dir = env_path.parent().unwrap_or_else(|| Path::new("."));
    let bootstrap_path = ironclaw_dir.join("bootstrap.json");
    let parsed = match read_bootstrap_json(&bootstrap_path) {
        Ok(Some(parsed)) => parsed,
        Ok(None) | Err(_) => return,
    };
    let pairs = extract_env_pairs(&parsed);
    if pairs.is_empty() {
        return;
    }
    if upsert_env_pairs(env_path, &pairs).is_err() {
        return;
    }
    let _ = rename_to_migrated(&bootstrap_path);
    eprintln!(
        "Migrated DATABASE_URL from bootstrap.json to {}",
        env_path.display()
    );
}

fn read_bootstrap_json(path: &Path) -> io::Result<Option<serde_json::Value>> {
    if !path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(path)?;
    serde_json::from_str(&content)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn extract_env_pairs(json: &serde_json::Value) -> Vec<EnvPair<'_>> {
    json.get("database_url")
        .and_then(serde_json::Value::as_str)
        .map(|url| {
            vec![EnvPair {
                key: KnownEnvKey::DatabaseUrl,
                value: Cow::Borrowed(url),
            }]
        })
        .unwrap_or_default()
}

fn quote_env_value(raw: &str) -> String {
    raw.replace('\\', "\\\\").replace('"', "\\\"")
}

fn upsert_env_pairs(env_path: &Path, pairs: &[EnvPair<'_>]) -> io::Result<()> {
    if let Some(parent) = env_path.parent()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        eprintln!("Warning: failed to create {}: {}", parent.display(), error);
        return Err(error);
    }

    let keys_being_written: std::collections::HashSet<&str> =
        pairs.iter().map(|pair| pair.key.as_str()).collect();
    let existing = match std::fs::read_to_string(env_path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => {
            eprintln!(
                "Warning: failed to migrate bootstrap.json to .env: {}",
                error
            );
            return Err(error);
        }
    };

    let mut result = String::new();
    for line in existing.lines() {
        let is_overwritten = line
            .split_once('=')
            .map(|(key, _)| keys_being_written.contains(key.trim()))
            .unwrap_or(false);
        if !is_overwritten {
            result.push_str(line);
            result.push('\n');
        }
    }

    for pair in pairs {
        result.push_str(&format!(
            "{}=\"{}\"\n",
            pair.key.as_str(),
            quote_env_value(pair.value.as_ref())
        ));
    }

    if let Err(error) = std::fs::write(env_path, result) {
        eprintln!(
            "Warning: failed to migrate bootstrap.json to .env: {}",
            error
        );
        return Err(error);
    }
    Ok(())
}

/// Errors that can occur during disk-to-DB migration.
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    #[error("Database error: {0}")]
    Database(String),
    #[error("IO error: {0}")]
    Io(String),
}

fn read_optional_json_file(path: &Path, file_name: &str) -> Option<serde_json::Value> {
    if !path.exists() {
        return None;
    }

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            tracing::warn!("Failed to read {}: {}", file_name, error);
            return None;
        }
    };

    match serde_json::from_str::<serde_json::Value>(&content) {
        Ok(value) => Some(value),
        Err(error) => {
            tracing::warn!("Failed to parse {}: {}", file_name, error);
            None
        }
    }
}

async fn migrate_json_sidecar(
    store: &dyn crate::db::Database,
    spec: &SidecarSpec<'_>,
) -> Result<(), MigrationError> {
    let Some(value) = read_optional_json_file(spec.path, spec.file_name) else {
        return Ok(());
    };

    store
        .set_setting(
            UserId::from(spec.user_id),
            SettingKey::from(spec.setting_key),
            &value,
        )
        .await
        .map_err(|error| MigrationError::Database(format!("{}: {}", spec.db_error_msg, error)))?;

    tracing::info!("{}", spec.success_msg);
    let _ = rename_to_migrated(spec.path);

    Ok(())
}

/// Renames `bootstrap.json` inside `ironclaw_dir` to `bootstrap.json.migrated`
/// if it exists.
///
/// Logs an `INFO` message when the rename succeeds. On failure,
/// [`rename_to_migrated`] emits a `WARN` log and the error is silently
/// discarded, preserving the existing warn-and-continue behaviour at all
/// bootstrap rename call sites.
pub(super) fn rename_legacy_bootstrap(ironclaw_dir: &Path) {
    let old_bootstrap = ironclaw_dir.join("bootstrap.json");
    if old_bootstrap.exists() && rename_to_migrated(&old_bootstrap).is_ok() {
        tracing::info!("Renamed old bootstrap.json to .migrated");
    }
}

fn read_legacy_state(path: &Path) -> Result<Option<serde_json::Value>, MigrationError> {
    if !path.exists() {
        return Ok(None);
    }

    let settings = crate::settings::Settings::load_from(path);
    Ok(serde_json::to_value(settings).ok())
}

async fn apply_migration_to_db(
    store: &dyn crate::db::Database,
    user_id: &str,
    legacy: &serde_json::Value,
    legacy_settings_path: &Path,
) -> Result<(), MigrationError> {
    let has_settings = store
        .has_settings(UserId::from(user_id))
        .await
        .map_err(|error| {
            MigrationError::Database(format!("Failed to check existing settings: {}", error))
        })?;
    if has_settings {
        tracing::info!("DB already has settings, renaming stale settings.json");
        return Ok(());
    }

    tracing::info!("Migrating disk settings to database...");

    let settings =
        serde_json::from_value::<crate::settings::Settings>(legacy.clone()).unwrap_or_default();
    let db_map = settings.to_db_map();
    if !db_map.is_empty() {
        store
            .set_all_settings(UserId::from(user_id), &db_map)
            .await
            .map_err(|error| {
                MigrationError::Database(format!("Failed to write settings to DB: {}", error))
            })?;
        tracing::info!("Migrated {} settings to database", db_map.len());
    }

    if let Some(ref url) = settings.database_url {
        save_database_url(url)
            .map_err(|error| MigrationError::Io(format!("Failed to write .env: {}", error)))?;
        tracing::info!("Wrote DATABASE_URL to {}", ironclaw_env_path().display());
    }

    let ironclaw_dir = legacy_settings_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let mcp_path = ironclaw_dir.join("mcp-servers.json");
    let mcp_spec = SidecarSpec {
        user_id,
        path: &mcp_path,
        file_name: "mcp-servers.json",
        setting_key: "mcp_servers",
        db_error_msg: "Failed to write MCP servers to DB",
        success_msg: "Migrated mcp-servers.json to database",
    };
    migrate_json_sidecar(store, &mcp_spec).await?;

    let session_path = ironclaw_dir.join("session.json");
    let session_spec = SidecarSpec {
        user_id,
        path: &session_path,
        file_name: "session.json",
        setting_key: "nearai.session_token",
        db_error_msg: "Failed to write session to DB",
        success_msg: "Migrated session.json to database",
    };
    migrate_json_sidecar(store, &session_spec).await?;

    rename_legacy_bootstrap(ironclaw_dir);

    tracing::info!("Disk-to-DB migration complete");
    Ok(())
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
    migrate_disk_to_db_from_dir(store, user_id, &ironclaw_dir).await
}

pub(super) async fn migrate_disk_to_db_from_dir(
    store: &dyn crate::db::Database,
    user_id: &str,
    ironclaw_dir: &Path,
) -> Result<(), MigrationError> {
    let legacy_settings_path = ironclaw_dir.join("settings.json");
    let Some(legacy) = read_legacy_state(&legacy_settings_path)? else {
        tracing::debug!("No legacy settings.json found, skipping disk-to-DB migration");
        return Ok(());
    };
    apply_migration_to_db(store, user_id, &legacy, &legacy_settings_path).await?;
    let _ = rename_to_migrated(&legacy_settings_path);
    Ok(())
}

/// Renames `path` to `<path>.migrated` as a safety-net marker indicating
/// the file has been processed by a migration pass.
///
/// Returns `Ok(())` on success. On failure the filesystem error is logged
/// at `WARN` level and returned to the caller; call sites that treat the
/// rename as non-fatal should discard the result with `let _ = …`.
pub(super) fn rename_to_migrated(path: &Path) -> io::Result<()> {
    let mut migrated = path.as_os_str().to_owned();
    migrated.push(".migrated");
    std::fs::rename(path, &migrated).map_err(|error| {
        tracing::warn!(
            "Failed to rename {} to .migrated: {}",
            path.display(),
            error
        );
        error
    })
}
