//! Null implementation of NativeSettingsStore for NullDatabase.

use std::collections::HashMap;

use crate::db::{SettingKey, UserId};
use crate::error::DatabaseError;
use crate::history::SettingRow;

use super::NullDatabase;

impl crate::db::NativeSettingsStore for NullDatabase {
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

    async fn delete_setting(
        &self,
        _user_id: UserId,
        _key: SettingKey,
    ) -> Result<bool, DatabaseError> {
        Ok(false)
    }

    async fn list_settings(&self, _user_id: UserId) -> Result<Vec<SettingRow>, DatabaseError> {
        Ok(vec![])
    }

    async fn set_setting(
        &self,
        _user_id: UserId,
        _key: SettingKey,
        _value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        Ok(())
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
