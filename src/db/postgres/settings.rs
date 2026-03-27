//! SettingsStore implementation for PostgreSQL backend.

use std::collections::HashMap;

use crate::db::NativeSettingsStore;
use crate::error::DatabaseError;
use crate::history::SettingRow;

use super::PgBackend;

impl NativeSettingsStore for PgBackend {
    crate::delegate_async! {
        to store;
        async fn get_setting(&self, user_id: &str, key: &str) -> Result<Option<serde_json::Value>, DatabaseError>;
        async fn get_setting_full(&self, user_id: &str, key: &str) -> Result<Option<SettingRow>, DatabaseError>;
        async fn set_setting(&self, user_id: &str, key: &str, value: &serde_json::Value) -> Result<(), DatabaseError>;
        async fn delete_setting(&self, user_id: &str, key: &str) -> Result<bool, DatabaseError>;
        async fn list_settings(&self, user_id: &str) -> Result<Vec<SettingRow>, DatabaseError>;
        async fn get_all_settings(&self, user_id: &str) -> Result<HashMap<String, serde_json::Value>, DatabaseError>;
        async fn set_all_settings(&self, user_id: &str, settings: &HashMap<String, serde_json::Value>) -> Result<(), DatabaseError>;
        async fn has_settings(&self, user_id: &str) -> Result<bool, DatabaseError>;
    }
}
