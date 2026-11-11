//! Legacy bootstrap and disk-to-database migration helpers.

use std::{io, path::Path};

use crate::bootstrap::{axinite_base_dir, axinite_env_path, save_database_url};
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

mod env_migration;

pub(crate) use env_migration::migrate_bootstrap_json_to_env;

/// Errors that can occur during disk-to-DB migration.
#[derive(Debug, thiserror::Error)]
pub enum MigrationError {
    /// A settings read or write failed. The message names the operation and
    /// carries the underlying database error.
    #[error("Database error: {0}")]
    Database(String),
    /// Writing the migrated `.env` file failed. The message carries the
    /// underlying I/O error.
    #[error("IO error: {0}")]
    Io(String),
}

fn read_optional_json_file(path: &Path, file_name: &str) -> Option<serde_json::Value> {
    if !path.exists() {
        return None;
    }

    let content = match ambient_fs::read_to_string(path) {
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
    store: &(impl crate::db::SettingsStore + ?Sized),
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

/// Renames `bootstrap.json` inside `axinite_dir` to `bootstrap.json.migrated`
/// if it exists.
///
/// Logs an `INFO` message when the rename succeeds. On failure,
/// [`rename_to_migrated`] emits a `WARN` log and the error is silently
/// discarded, preserving the existing warn-and-continue behaviour at all
/// bootstrap rename call sites.
pub(super) fn rename_legacy_bootstrap(axinite_dir: &Path) {
    let old_bootstrap = axinite_dir.join("bootstrap.json");
    match old_bootstrap.try_exists() {
        Ok(true) => {
            if rename_to_migrated(&old_bootstrap).is_ok() {
                tracing::info!("Renamed old bootstrap.json to .migrated");
            }
        }
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(
                "Failed to inspect old bootstrap.json before migration rename: {}",
                error
            );
        }
    }
}

/// Read the pre-migration `settings.json` as a JSON object.
///
/// Returns `Ok(None)` when the file is absent, so a first run with no legacy
/// state is not an error. A file whose metadata cannot be read, or that exists
/// but cannot be parsed, is reported as [`MigrationError::Io`] because a silent
/// default would discard the user's settings during the one-way migration into
/// the database.
fn read_legacy_state(path: &Path) -> Result<Option<serde_json::Value>, MigrationError> {
    // `try_exists` distinguishes "absent" from "metadata unreadable"; a plain
    // `exists()` would report both as absent and silently skip the migration.
    if !path
        .try_exists()
        .map_err(|error| MigrationError::Io(error.to_string()))?
    {
        return Ok(None);
    }

    let settings = crate::settings::Settings::load_from(path)
        .map_err(|error| MigrationError::Io(error.to_string()))?;
    Ok(serde_json::to_value(settings).ok())
}

async fn apply_migration_to_db(
    store: &(impl crate::db::SettingsStore + ?Sized),
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
        tracing::info!("Wrote DATABASE_URL to {}", axinite_env_path().display());
    }

    let axinite_dir = legacy_settings_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let mcp_path = axinite_dir.join("mcp-servers.json");
    let mcp_spec = SidecarSpec {
        user_id,
        path: &mcp_path,
        file_name: "mcp-servers.json",
        setting_key: "mcp_servers",
        db_error_msg: "Failed to write MCP servers to DB",
        success_msg: "Migrated mcp-servers.json to database",
    };
    migrate_json_sidecar(store, &mcp_spec).await?;

    let session_path = axinite_dir.join("session.json");
    let session_spec = SidecarSpec {
        user_id,
        path: &session_path,
        file_name: "session.json",
        setting_key: "nearai.session_token",
        db_error_msg: "Failed to write session to DB",
        success_msg: "Migrated session.json to database",
    };
    migrate_json_sidecar(store, &session_spec).await?;

    rename_legacy_bootstrap(axinite_dir);

    tracing::info!("Disk-to-DB migration complete");
    Ok(())
}

/// One-time migration of legacy `~/.axinite/settings.json` into the database.
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
    let axinite_dir = axinite_base_dir();
    migrate_disk_to_db_from_dir(store, user_id, &axinite_dir).await
}

/// Path-injected disk-to-database migration entrypoint.
///
/// Migrates legacy disk data from `axinite_dir` into `store` for `user_id`.
/// Higher-level migration orchestration calls this with a concrete
/// [`crate::db::Database`] implementor, the current user identifier, and the
/// Axinite base directory whose legacy files should be inspected.
///
/// Rename operations that mark legacy files as `.migrated` are best-effort:
/// failures are logged by the lower-level rename helper and do not cause this
/// migration to fail. Database and read/write failures that should stop the
/// migration are returned as [`MigrationError`].
pub(super) async fn migrate_disk_to_db_from_dir(
    store: &(impl crate::db::SettingsStore + ?Sized),
    user_id: &str,
    axinite_dir: &Path,
) -> Result<(), MigrationError> {
    let legacy_settings_path = axinite_dir.join("settings.json");
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
    ambient_fs::rename(path, &migrated).map_err(|error| {
        tracing::warn!(
            "Failed to rename {} to .migrated: {}",
            path.display(),
            error
        );
        error
    })
}
