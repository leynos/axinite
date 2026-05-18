//! Shared fixtures for bootstrap migration tests.

use std::collections::HashMap;
use std::sync::Mutex;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tempfile::{TempDir, tempdir};

use crate::db::{DbFuture, SettingKey, SettingsStore, UserId};
use crate::error::DatabaseError;
use crate::history::SettingRow;

#[derive(Clone, Copy)]
pub(super) enum RenameSetup {
    ExistingFile,
    MissingFile,
    #[cfg(unix)]
    ReadOnlyDirectory,
}

pub(super) struct RenameFixture {
    pub(super) dir: TempDir,
    pub(super) path: std::path::PathBuf,
    #[cfg(unix)]
    original_dir_permissions: Option<std::fs::Permissions>,
}

#[derive(Debug, Default)]
pub(super) struct MigrationStoreState {
    pub(super) has_settings_calls: usize,
    pub(super) set_all_settings_calls: usize,
    pub(super) set_setting_calls: usize,
    pub(super) captured_settings: HashMap<String, serde_json::Value>,
}

#[derive(Debug)]
pub(super) struct MigrationStore {
    has_settings_result: Result<bool, &'static str>,
    set_all_settings_result: Result<(), &'static str>,
    state: Mutex<MigrationStoreState>,
}

impl MigrationStore {
    pub(super) fn new(has_settings_result: Result<bool, &'static str>) -> Self {
        Self {
            has_settings_result,
            set_all_settings_result: Ok(()),
            state: Mutex::new(MigrationStoreState::default()),
        }
    }

    pub(super) fn with_set_all_error() -> Self {
        Self {
            has_settings_result: Ok(false),
            set_all_settings_result: Err("forced set_all_settings failure"),
            state: Mutex::new(MigrationStoreState::default()),
        }
    }

    pub(super) fn state(&self) -> std::sync::MutexGuard<'_, MigrationStoreState> {
        self.state.lock().expect("migration store state lock")
    }
}

impl SettingsStore for MigrationStore {
    fn get_setting<'a>(
        &'a self,
        _user_id: UserId,
        _key: SettingKey,
    ) -> DbFuture<'a, Result<Option<serde_json::Value>, DatabaseError>> {
        Box::pin(async { Ok(None) })
    }

    fn get_setting_full<'a>(
        &'a self,
        _user_id: UserId,
        _key: SettingKey,
    ) -> DbFuture<'a, Result<Option<SettingRow>, DatabaseError>> {
        Box::pin(async { Ok(None) })
    }

    fn set_setting<'a>(
        &'a self,
        _user_id: UserId,
        _key: SettingKey,
        _value: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(async {
            self.state().set_setting_calls += 1;
            Ok(())
        })
    }

    fn delete_setting<'a>(
        &'a self,
        _user_id: UserId,
        _key: SettingKey,
    ) -> DbFuture<'a, Result<bool, DatabaseError>> {
        Box::pin(async { Ok(false) })
    }

    fn list_settings<'a>(
        &'a self,
        _user_id: UserId,
    ) -> DbFuture<'a, Result<Vec<SettingRow>, DatabaseError>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn get_all_settings<'a>(
        &'a self,
        _user_id: UserId,
    ) -> DbFuture<'a, Result<HashMap<String, serde_json::Value>, DatabaseError>> {
        Box::pin(async { Ok(HashMap::new()) })
    }

    fn set_all_settings<'a>(
        &'a self,
        _user_id: UserId,
        settings: &'a HashMap<String, serde_json::Value>,
    ) -> DbFuture<'a, Result<(), DatabaseError>> {
        Box::pin(async move {
            let mut state = self.state();
            state.set_all_settings_calls += 1;
            state.captured_settings = settings.clone();
            drop(state);

            self.set_all_settings_result
                .map_err(|message| DatabaseError::Query(message.to_string()))
        })
    }

    fn has_settings<'a>(&'a self, _user_id: UserId) -> DbFuture<'a, Result<bool, DatabaseError>> {
        Box::pin(async {
            self.state().has_settings_calls += 1;
            self.has_settings_result
                .map_err(|message| DatabaseError::Query(message.to_string()))
        })
    }
}

impl RenameFixture {
    pub(super) fn prepare(&mut self, setup: RenameSetup) {
        match setup {
            RenameSetup::ExistingFile => self.write_legacy_file(),
            RenameSetup::MissingFile => {}
            #[cfg(unix)]
            RenameSetup::ReadOnlyDirectory => {
                self.write_legacy_file();
                self.make_dir_read_only();
            }
        }
    }

    fn write_legacy_file(&self) {
        std::fs::write(&self.path, "{}").expect("write legacy settings file");
    }

    #[cfg(unix)]
    fn make_dir_read_only(&mut self) {
        self.original_dir_permissions = Some(
            std::fs::metadata(self.dir.path())
                .expect("read directory metadata")
                .permissions(),
        );
        std::fs::set_permissions(self.dir.path(), std::fs::Permissions::from_mode(0o555))
            .expect("make directory read-only");
    }

    pub(super) fn migrated_path(&self) -> std::path::PathBuf {
        let mut migrated = self.path.as_os_str().to_owned();
        migrated.push(".migrated");
        migrated.into()
    }
}

#[cfg(unix)]
impl Drop for RenameFixture {
    fn drop(&mut self) {
        if let Some(permissions) = self.original_dir_permissions.take() {
            let _ = std::fs::set_permissions(self.dir.path(), permissions);
        }
    }
}

pub(super) fn rename_fixture() -> RenameFixture {
    let dir = tempdir().expect("create temp dir for rename test");
    let path = dir.path().join("settings.json");
    RenameFixture {
        dir,
        path,
        #[cfg(unix)]
        original_dir_permissions: None,
    }
}

pub(super) fn write_legacy_settings(dir: &TempDir) -> std::path::PathBuf {
    let settings_path = dir.path().join("settings.json");
    std::fs::write(
        &settings_path,
        serde_json::json!({
            "onboard_completed": true,
            "database_backend": "libsql"
        })
        .to_string(),
    )
    .expect("write legacy settings.json");
    settings_path
}

pub(super) fn assert_store_state(
    store: &MigrationStore,
    expected_has_settings: usize,
    expected_set_all_settings: usize,
) {
    let state = store.state();
    assert_eq!(
        state.has_settings_calls, expected_has_settings,
        "unexpected has_settings call count"
    );
    assert_eq!(
        state.set_all_settings_calls, expected_set_all_settings,
        "unexpected set_all_settings call count"
    );
    assert_eq!(state.set_setting_calls, 0, "set_setting must not be called");
}

pub(super) fn assert_legacy_file_renamed(dir: &TempDir) {
    assert!(
        !dir.path().join("settings.json").exists(),
        "settings.json must have been renamed away"
    );
    assert!(
        dir.path().join("settings.json.migrated").exists(),
        "settings.json.migrated must exist"
    );
}

pub(super) fn assert_legacy_file_not_renamed(dir: &TempDir) {
    assert!(
        dir.path().join("settings.json").exists(),
        "settings.json must still be present after a failed migration"
    );
    assert!(
        !dir.path().join("settings.json.migrated").exists(),
        "settings.json.migrated must not exist after a failed migration"
    );
}
