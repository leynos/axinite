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
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserId(String);

impl UserId {
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

/// Strongly typed settings key for settings-store methods.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SettingKey(String);

impl SettingKey {
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
    fn get_setting<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
    ) -> DbFuture<'a, Result<Option<serde_json::Value>, DatabaseError>>;
    fn get_setting_full<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
    ) -> DbFuture<'a, Result<Option<SettingRow>, DatabaseError>>;
    fn set_setting<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
        value: &'a serde_json::Value,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn delete_setting<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
    ) -> DbFuture<'a, Result<bool, DatabaseError>>;
    fn list_settings<'a>(
        &'a self,
        user_id: UserId,
    ) -> DbFuture<'a, Result<Vec<SettingRow>, DatabaseError>>;
    fn get_all_settings<'a>(
        &'a self,
        user_id: UserId,
    ) -> DbFuture<'a, Result<HashMap<String, serde_json::Value>, DatabaseError>>;
    fn set_all_settings<'a>(
        &'a self,
        user_id: UserId,
        settings: &'a HashMap<String, serde_json::Value>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn has_settings<'a>(&'a self, user_id: UserId) -> DbFuture<'a, Result<bool, DatabaseError>>;
}

/// Native async sibling trait for concrete settings-store implementations.
pub trait NativeSettingsStore: Send + Sync {
    fn get_setting<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
    ) -> impl Future<Output = Result<Option<serde_json::Value>, DatabaseError>> + Send + 'a;
    fn get_setting_full<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
    ) -> impl Future<Output = Result<Option<SettingRow>, DatabaseError>> + Send + 'a;
    fn set_setting<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
        value: &'a serde_json::Value,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn delete_setting<'a>(
        &'a self,
        user_id: UserId,
        key: SettingKey,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
    fn list_settings<'a>(
        &'a self,
        user_id: UserId,
    ) -> impl Future<Output = Result<Vec<SettingRow>, DatabaseError>> + Send + 'a;
    fn get_all_settings<'a>(
        &'a self,
        user_id: UserId,
    ) -> impl Future<Output = Result<HashMap<String, serde_json::Value>, DatabaseError>> + Send + 'a;
    fn set_all_settings<'a>(
        &'a self,
        user_id: UserId,
        settings: &'a HashMap<String, serde_json::Value>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn has_settings<'a>(
        &'a self,
        user_id: UserId,
    ) -> impl Future<Output = Result<bool, DatabaseError>> + Send + 'a;
}
