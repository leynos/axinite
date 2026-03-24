//! SettingsStore implementation for PostgreSQL backend.

use std::collections::HashMap;

use crate::db::{NativeSettingsStore, SettingKey, UserId};
use crate::error::DatabaseError;
use crate::history::SettingRow;

use super::PgBackend;

macro_rules! delegate_to_store {
    (
        $(
            async fn $method:ident ( &self $(, $arg:ident : $ty:ty)* ) -> $ret:ty ;
        )*
    ) => {
        $(
            async fn $method ( &self $(, $arg : $ty )* ) -> $ret {
                self.store . $method ( $( $arg ),* ) .await
            }
        )*
    };
}

impl NativeSettingsStore for PgBackend {
    delegate_to_store! {
        async fn get_setting(&self, user_id: UserId<'_>, key: SettingKey<'_>) -> Result<Option<serde_json::Value>, DatabaseError>;
        async fn get_setting_full(&self, user_id: UserId<'_>, key: SettingKey<'_>) -> Result<Option<SettingRow>, DatabaseError>;
        async fn set_setting(&self, user_id: UserId<'_>, key: SettingKey<'_>, value: &serde_json::Value) -> Result<(), DatabaseError>;
        async fn delete_setting(&self, user_id: UserId<'_>, key: SettingKey<'_>) -> Result<bool, DatabaseError>;
        async fn list_settings(&self, user_id: UserId<'_>) -> Result<Vec<SettingRow>, DatabaseError>;
        async fn get_all_settings(&self, user_id: UserId<'_>) -> Result<HashMap<String, serde_json::Value>, DatabaseError>;
        async fn set_all_settings(&self, user_id: UserId<'_>, settings: &HashMap<String, serde_json::Value>) -> Result<(), DatabaseError>;
        async fn has_settings(&self, user_id: UserId<'_>) -> Result<bool, DatabaseError>;
    }
}
