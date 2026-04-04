//! Settings persistence traits.
//!
//! Defines the dyn-safe [`SettingsStore`] and its native-async sibling
//! [`NativeSettingsStore`] for per-user key-value settings storage.

use core::fmt;
use std::{collections::HashMap, future::Future};

use crate::db::params::DbFuture;
use crate::error::DatabaseError;
use crate::history::SettingRow;

/// Strongly typed user identifier for settings-store methods.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct UserId(String);

impl UserId {
    /// Return a borrowed `&str` view of the inner user identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for UserId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for UserId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<&str> for UserId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for UserId {
    fn eq(&self, other: &String) -> bool {
        self.0 == *other
    }
}

/// Strongly typed settings key for settings-store methods.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SettingKey(String);

impl SettingKey {
    /// Return a borrowed `&str` view of the inner setting key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SettingKey {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl From<String> for SettingKey {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl fmt::Display for SettingKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Object-safe persistence surface for per-user key-value settings.
pub trait SettingsStore: Send + Sync {
    /// Load one JSON setting value for `user_id` and `key`.
    fn get_setting<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
    ) -> DbFuture<'a, Result<Option<serde_json::Value>, DatabaseError>>;
    /// Load one full persisted setting row for `user_id` and `key`.
    fn get_setting_full<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
    ) -> DbFuture<'a, Result<Option<SettingRow>, DatabaseError>>;
    /// Insert or replace the JSON value stored at `(user_id, key)`.
    fn set_setting<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
        value: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Delete the setting row at `(user_id, key)`.
    fn delete_setting<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
    ) -> DbFuture<'a, Result<bool, DatabaseError>>;
    /// List every persisted setting row for `user_id`.
    fn list_settings<'a>(
        &'a self,
        user_id: UserId,
    ) -> DbFuture<'a, Result<Vec<SettingRow>, DatabaseError>>;
    /// Load all settings for `user_id` as a key-to-value map.
    fn get_all_settings<'a>(
        &'a self,
        user_id: UserId,
    ) -> DbFuture<'a, Result<HashMap<SettingKey, serde_json::Value>, DatabaseError>>;
    /// Replace the full settings set for `user_id` with `settings`.
    fn set_all_settings<'a>(
        &'a self,
        user_id: UserId,
        settings: &'a HashMap<SettingKey, serde_json::Value>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Report whether any settings exist for `user_id`.
    fn has_settings<'a>(&'a self, user_id: UserId) -> DbFuture<'a, Result<bool, DatabaseError>>;
}

/// Native async sibling trait for concrete settings-store implementations.
pub trait NativeSettingsStore: Send + Sync {
    /// Load one JSON setting value for `user_id` and `key`.
    fn get_setting<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
    ) -> impl Future<Output = Result<Option<serde_json::Value>, DatabaseError>> + Send + 'a;
    /// Load one full persisted setting row for `user_id` and `key`.
    fn get_setting_full<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
    ) -> impl Future<Output = Result<Option<SettingRow>, DatabaseError>> + Send + 'a;
    /// Insert or replace the JSON value stored at `(user_id, key)`.
    fn set_setting<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
        value: &'a serde_json::Value,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Delete the setting row at `(user_id, key)`.
    fn delete_setting<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
    /// List every persisted setting row for `user_id`.
    fn list_settings<'a>(
        &'a self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<SettingRow>, DatabaseError>> + Send + 'a;
    /// Load all settings for `user_id` as a key-to-value map.
    fn get_all_settings<'a>(
        &'a self,
        user_id: UserId,
    ) -> impl Future<Output = Result<HashMap<SettingKey, serde_json::Value>, DatabaseError>> + Send + 'a;
    /// Replace the full settings set for `user_id` with `settings`.
    fn set_all_settings<'a>(
        &'a self,
        user_id: UserId,
        settings: &'a HashMap<SettingKey, serde_json::Value>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Report whether any settings exist for `user_id`.
    fn has_settings<'a>(
        &'a self,
        user_id: UserId,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
}
