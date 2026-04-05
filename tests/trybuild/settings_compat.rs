use std::collections::HashMap;

use ironclaw::db::settings::{
    NativeSettingsStore, SettingKey, SettingsStore, UserId,
};
use ironclaw::error::DatabaseError;
use ironclaw::history::SettingRow;

struct DummySettingsStore;

impl NativeSettingsStore for DummySettingsStore {
    async fn get_setting(
        &self,
        _user_id: UserId,
        _key: SettingKey,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        Ok(None)
    }

    async fn get_setting_full(
        &self,
        _user_id: UserId,
        _key: SettingKey,
    ) -> Result<Option<SettingRow>, DatabaseError> {
        Ok(None)
    }

    async fn set_setting(
        &self,
        _user_id: UserId,
        _key: SettingKey,
        _value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn delete_setting(
        &self,
        _user_id: UserId,
        _key: SettingKey,
    ) -> Result<bool, DatabaseError> {
        Ok(false)
    }

    async fn list_settings(
        &self,
        _user_id: UserId,
    ) -> Result<Vec<SettingRow>, DatabaseError> {
        Ok(Vec::new())
    }

    async fn get_all_settings(
        &self,
        _user_id: UserId,
    ) -> Result<HashMap<String, serde_json::Value>, DatabaseError> {
        Ok(HashMap::new())
    }

    async fn set_all_settings(
        &self,
        _user_id: UserId,
        _settings: &HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn has_settings(&self, _user_id: UserId) -> Result<bool, DatabaseError> {
        Ok(false)
    }
}

fn assert_compat_path<T: SettingsStore>(store: &T) {
    let value = serde_json::json!("dark");
    let _ = SettingsStore::set_setting(
        store,
        UserId::from("compat-user"),
        SettingKey::from("theme"),
        &value,
    );
}

fn main() {
    let store = DummySettingsStore;
    assert_compat_path(&store);
}
