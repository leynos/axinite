//! Settings persistence traits.
//!
//! Defines the dyn-safe [`SettingsStore`] and its native-async sibling
//! [`NativeSettingsStore`] for per-user key-value settings storage.

use std::collections::HashMap;
use std::future::Future;

use crate::db::params::DbFuture;
use crate::error::DatabaseError;
use crate::history::SettingRow;

/// Object-safe persistence surface for per-user key-value settings.
pub trait SettingsStore: Send + Sync {
    fn get_setting<'a>(
        &'a self,
        user_id: &'a str,
        key: &'a str,
    ) -> DbFuture<'a, Result<Option<serde_json::Value>, DatabaseError>>;
    fn get_setting_full<'a>(
        &'a self,
        user_id: &'a str,
        key: &'a str,
    ) -> DbFuture<'a, Result<Option<SettingRow>, DatabaseError>>;
    fn set_setting<'a>(
        &'a self,
        user_id: &'a str,
        key: &'a str,
        value: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn delete_setting<'a>(
        &'a self,
        user_id: &'a str,
        key: &'a str,
    ) -> DbFuture<'a, Result<bool, DatabaseError>>;
    fn list_settings<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<Vec<SettingRow>, DatabaseError>>;
    fn get_all_settings<'a>(
        &'a self,
        user_id: &'a str,
    ) -> DbFuture<'a, Result<HashMap<String, serde_json::Value>, DatabaseError>>;
    fn set_all_settings<'a>(
        &'a self,
        user_id: &'a str,
        settings: &'a HashMap<String, serde_json::Value>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn has_settings<'a>(&'a self, user_id: &'a str) -> DbFuture<'a, Result<bool, DatabaseError>>;
}

/// Native async sibling trait for concrete settings-store implementations.
pub trait NativeSettingsStore: Send + Sync {
    fn get_setting<'a>(
        &'a self,
        user_id: &'a str,
        key: &'a str,
    ) -> impl Future<Output = Result<Option<serde_json::Value>, DatabaseError>> + Send + 'a;
    fn get_setting_full<'a>(
        &'a self,
        user_id: &'a str,
        key: &'a str,
    ) -> impl Future<Output = Result<Option<SettingRow>, DatabaseError>> + Send + 'a;
    fn set_setting<'a>(
        &'a self,
        user_id: &'a str,
        key: &'a str,
        value: &'a serde_json::Value,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn delete_setting<'a>(
        &'a self,
        user_id: &'a str,
        key: &'a str,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
    fn list_settings<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Vec<SettingRow>, DatabaseError>> + Send + 'a;
    fn get_all_settings<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<HashMap<String, serde_json::Value>, DatabaseError>> + Send + 'a;
    fn set_all_settings<'a>(
        &'a self,
        user_id: &'a str,
        settings: &'a HashMap<String, serde_json::Value>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn has_settings<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
}
