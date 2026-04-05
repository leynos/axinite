//! SettingsStore implementation for PostgreSQL backend.

use std::collections::HashMap;

use crate::db::{NativeSettingsStore, SettingKey, UserId};
use crate::error::DatabaseError;
use crate::history::SettingRow;

use super::PgBackend;

impl NativeSettingsStore for PgBackend {
    crate::db::delegate_async! {
        to store;
        async fn get_setting(&self, user_id: UserId, key: SettingKey) -> Result<Option<serde_json::Value>, DatabaseError>;
        async fn get_setting_full(&self, user_id: UserId, key: SettingKey) -> Result<Option<SettingRow>, DatabaseError>;
        async fn set_setting(&self, user_id: UserId, key: SettingKey, value: &serde_json::Value) -> Result<(), DatabaseError>;
        async fn delete_setting(&self, user_id: UserId, key: SettingKey) -> Result<bool, DatabaseError>;
        async fn list_settings(&self, user_id: UserId) -> Result<Vec<SettingRow>, DatabaseError>;
        async fn get_all_settings(&self, user_id: UserId) -> Result<HashMap<String, serde_json::Value>, DatabaseError>;
        async fn set_all_settings(&self, user_id: UserId, settings: &HashMap<String, serde_json::Value>) -> Result<(), DatabaseError>;
        async fn has_settings(&self, user_id: UserId) -> Result<bool, DatabaseError>;
    }
}
