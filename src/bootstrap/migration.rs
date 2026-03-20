//! Legacy bootstrap and disk-to-database migration helpers.

use std::path::Path;

use crate::bootstrap::{ironclaw_base_dir, ironclaw_env_path, save_database_url};

/// If `bootstrap.json` exists, pull `database_url` out of it and write `.env`.
pub(crate) fn migrate_bootstrap_json_to_env(env_path: &Path) {
    let ironclaw_dir = env_path.parent().unwrap_or_else(|| Path::new("."));
    let bootstrap_path = ironclaw_dir.join("bootstrap.json");

    if !bootstrap_path.exists() {
        return;
    }

    let content = match std::fs::read_to_string(&bootstrap_path) {
        Ok(content) => content,
        Err(_) => return,
    };

    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(value) => value,
        Err(_) => return,
    };

    if let Some(url) = parsed.get("database_url").and_then(|value| value.as_str()) {
        if let Some(parent) = env_path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            eprintln!("Warning: failed to create {}: {}", parent.display(), error);
            return;
        }
        if let Err(error) = std::fs::write(env_path, format!("DATABASE_URL=\"{}\"\n", url)) {
            eprintln!(
                "Warning: failed to migrate bootstrap.json to .env: {}",
                error
            );
            return;
        }
        rename_to_migrated(&bootstrap_path);
        eprintln!(
            "Migrated DATABASE_URL from bootstrap.json to {}",
            env_path.display()
        );
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
    user_id: &str,
    path: &Path,
    file_name: &str,
    setting_key: &str,
    db_error_message: &str,
    success_message: &str,
) -> Result<(), MigrationError> {
    let Some(value) = read_optional_json_file(path, file_name) else {
        return Ok(());
    };

    store
        .set_setting(user_id, setting_key, &value)
        .await
        .map_err(|error| MigrationError::Database(format!("{}: {}", db_error_message, error)))?;
    tracing::info!("{}", success_message);
    rename_to_migrated(path);

    Ok(())
}

fn rename_legacy_bootstrap(ironclaw_dir: &Path) {
    let old_bootstrap = ironclaw_dir.join("bootstrap.json");
    if old_bootstrap.exists() {
        rename_to_migrated(&old_bootstrap);
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
    let has_settings = store.has_settings(user_id).await.map_err(|error| {
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
            .set_all_settings(user_id, &db_map)
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
    migrate_json_sidecar(
        store,
        user_id,
        &mcp_path,
        "mcp-servers.json",
        "mcp_servers",
        "Failed to write MCP servers to DB",
        "Migrated mcp-servers.json to database",
    )
    .await?;

    let session_path = ironclaw_dir.join("session.json");
    migrate_json_sidecar(
        store,
        user_id,
        &session_path,
        "session.json",
        "nearai.session_token",
        "Failed to write session to DB",
        "Migrated session.json to database",
    )
    .await?;

    rename_legacy_bootstrap(ironclaw_dir);

    tracing::info!("Disk-to-DB migration complete");
    Ok(())
}

fn mark_legacy_migrated(path: &Path) -> Result<(), MigrationError> {
    rename_to_migrated(path);
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
    let legacy_settings_path = ironclaw_dir.join("settings.json");
    let Some(legacy) = read_legacy_state(&legacy_settings_path)? else {
        tracing::debug!("No legacy settings.json found, skipping disk-to-DB migration");
        return Ok(());
    };
    apply_migration_to_db(store, user_id, &legacy, &legacy_settings_path).await?;
    mark_legacy_migrated(&legacy_settings_path)?;
    Ok(())
}

/// Rename a file to `<name>.migrated` as a safety net.
fn rename_to_migrated(path: &Path) {
    let mut migrated = path.as_os_str().to_owned();
    migrated.push(".migrated");
    if let Err(error) = std::fs::rename(path, &migrated) {
        tracing::warn!(
            "Failed to rename {} to .migrated: {}",
            path.display(),
            error
        );
    }
}
