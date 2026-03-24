//! SettingsStore implementation for PostgreSQL backend.

use std::collections::HashMap;

use crate::db::{NativeSettingsStore, SettingKey, UserId};
use crate::error::DatabaseError;
use crate::history::SettingRow;

use super::PgBackend;

impl NativeSettingsStore for PgBackend {
    async fn get_setting(
        &self,
        user_id: UserId<'_>,
        key: SettingKey<'_>,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        self.store.get_setting(user_id.as_str(), key.as_str()).await
    }

    async fn get_setting_full(
        &self,
        user_id: UserId<'_>,
        key: SettingKey<'_>,
    ) -> Result<Option<SettingRow>, DatabaseError> {
        self.store
            .get_setting_full(user_id.as_str(), key.as_str())
            .await
    }

    async fn set_setting(
        &self,
        user_id: UserId<'_>,
        key: SettingKey<'_>,
        value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        self.store
            .set_setting(user_id.as_str(), key.as_str(), value)
            .await
    }

    async fn delete_setting(
        &self,
        user_id: UserId<'_>,
        key: SettingKey<'_>,
    ) -> Result<bool, DatabaseError> {
        self.store
            .delete_setting(user_id.as_str(), key.as_str())
            .await
    }

    async fn list_settings(&self, user_id: UserId<'_>) -> Result<Vec<SettingRow>, DatabaseError> {
        self.store.list_settings(user_id.as_str()).await
    }

    async fn get_all_settings(
        &self,
        user_id: UserId<'_>,
    ) -> Result<HashMap<String, serde_json::Value>, DatabaseError> {
        self.store.get_all_settings(user_id.as_str()).await
    }

    async fn set_all_settings(
        &self,
        user_id: UserId<'_>,
        settings: &HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        self.store.set_all_settings(user_id.as_str(), settings).await
    }

    async fn has_settings(&self, user_id: UserId<'_>) -> Result<bool, DatabaseError> {
        self.store.has_settings(user_id.as_str()).await
    }
}
