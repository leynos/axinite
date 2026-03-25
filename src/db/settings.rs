//! Settings storage abstractions for user preferences and configuration.
//!
//! This module provides the settings-related types and traits for persisting
//! user configuration across sessions. It follows the ADR-006 dyn/native boundary
//! pattern with both object-safe and native-async trait variants.
//!
//! # Type Newtypes
//!
//! - [`UserId`]: Strongly-typed user identifier
//! - [`SettingKey`]: Strongly-typed setting key
//!
//! # Traits
//!
//! - [`SettingsStore`]: Object-safe trait using boxed futures for trait objects
//! - [`NativeSettingsStore`]: Native async trait with RPITIT for concrete implementations
//!
//! The blanket implementation automatically bridges `NativeSettingsStore` to `SettingsStore`.

use std::collections::HashMap;
use std::future::Future;

use crate::db::DbFuture;
use crate::error::DatabaseError;
use crate::history::SettingRow;

/// User identifier newtype for settings store methods.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct UserId(String);

impl UserId {
    /// Returns the user identifier as a string slice.
    ///
    /// This provides read-only access to the underlying user ID string
    /// for use with external APIs or database queries.
    ///
    /// # Example
    /// ```ignore
    /// let user_id = UserId::from("alice");
    /// assert_eq!(user_id.as_str(), "alice");
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for UserId {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// Setting key newtype for settings store methods.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SettingKey(String);

impl SettingKey {
    /// Returns the setting key as a string slice.
    ///
    /// This provides read-only access to the underlying setting key string
    /// for use with external APIs or database queries.
    ///
    /// # Example
    /// ```ignore
    /// let key = SettingKey::from("theme");
    /// assert_eq!(key.as_str(), "theme");
    /// ```
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SettingKey {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// Object-safe persistence surface for user settings.
///
/// This trait provides the dyn-safe boundary for settings storage operations,
/// enabling trait-object usage (e.g., `Arc<dyn SettingsStore>`). It uses boxed
/// futures (`DbFuture<'a, T>`) to maintain object safety.
///
/// Companion trait: [`NativeSettingsStore`] provides the same API using native
/// async traits (RPITIT). A blanket adapter automatically bridges implementations
/// of `NativeSettingsStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support concurrent access.
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

macro_rules! settings_delegate {
    (uid_key, $name:ident ( $($extra_arg:ident : $extra_ty:ty),* ) -> $ret:ty) => {
        fn $name<'a>(
            &'a self,
            user_id: UserId,
            key: SettingKey,
            $( $extra_arg: $extra_ty, )*
        ) -> DbFuture<'a, $ret> {
            Box::pin(NativeSettingsStore::$name(
                self,
                user_id,
                key,
                $( $extra_arg, )*
            ))
        }
    };
    (uid, $name:ident ( $($extra_arg:ident : $extra_ty:ty),* ) -> $ret:ty) => {
        fn $name<'a>(
            &'a self,
            user_id: UserId,
            $( $extra_arg: $extra_ty, )*
        ) -> DbFuture<'a, $ret> {
            Box::pin(NativeSettingsStore::$name(
                self,
                user_id,
                $( $extra_arg, )*
            ))
        }
    };
}

impl<T> SettingsStore for T
where
    T: NativeSettingsStore + Send + Sync,
{
    settings_delegate!(uid_key, get_setting()      -> Result<Option<serde_json::Value>, DatabaseError>);
    settings_delegate!(uid_key, get_setting_full() -> Result<Option<SettingRow>, DatabaseError>);
    settings_delegate!(uid_key, set_setting(value: &'a serde_json::Value) -> Result<(), DatabaseError>);
    settings_delegate!(uid_key, delete_setting()   -> Result<bool, DatabaseError>);
    settings_delegate!(uid, list_settings()        -> Result<Vec<SettingRow>, DatabaseError>);
    settings_delegate!(uid, get_all_settings()     -> Result<HashMap<String, serde_json::Value>, DatabaseError>);
    settings_delegate!(uid, set_all_settings(settings: &'a HashMap<String, serde_json::Value>) -> Result<(), DatabaseError>);
    settings_delegate!(uid, has_settings()         -> Result<bool, DatabaseError>);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asserts the standard string-newtype contract for a type that implements
    /// `From<&str>`, `Clone`, `PartialEq`, `Eq`, `Hash`, and `as_str() -> &str`.
    macro_rules! assert_string_newtype_contract {
        ($Type:ty, $primary:expr, $other:expr) => {{
            // From + as_str
            let value = <$Type>::from($primary);
            assert_eq!(value.as_str(), $primary);

            // Clone
            let cloned = value.clone();
            assert_eq!(cloned.as_str(), $primary);

            // PartialEq
            assert_eq!(value, cloned);
            let other = <$Type>::from($other);
            assert_ne!(value, other);

            // Hash (via HashSet round-trip)
            let mut set = std::collections::HashSet::new();
            set.insert(value.clone());
            assert!(set.contains(&value));
            assert!(!set.contains(&other));
        }};
    }

    #[test]
    fn test_user_id_newtype() {
        assert_string_newtype_contract!(UserId, "test_user", "other_user");
    }

    #[test]
    fn test_setting_key_newtype() {
        assert_string_newtype_contract!(SettingKey, "api_key", "other_key");
    }

    // Dummy implementation for testing settings_delegate macro
    struct DummySettingsStore;

    impl NativeSettingsStore for DummySettingsStore {
        async fn get_setting(
            &self,
            user_id: UserId,
            key: SettingKey,
        ) -> Result<Option<serde_json::Value>, DatabaseError> {
            if user_id.as_str() == "test_user" && key.as_str() == "test_key" {
                Ok(Some(serde_json::json!("test_value")))
            } else {
                Ok(None)
            }
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
            Ok(true)
        }

        async fn list_settings(&self, _user_id: UserId) -> Result<Vec<SettingRow>, DatabaseError> {
            Ok(vec![])
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

    #[tokio::test]
    async fn test_settings_delegate_macro() {
        let store = DummySettingsStore;

        // Test that the blanket impl (via settings_delegate! macro) correctly accepts newtypes
        let result =
            SettingsStore::get_setting(&store, "test_user".into(), "test_key".into()).await;
        let value = result.expect("get_setting for test_user/test_key failed");
        assert_eq!(value, Some(serde_json::json!("test_value")));

        // Test with non-matching values
        let result =
            SettingsStore::get_setting(&store, "wrong_user".into(), "test_key".into()).await;
        let value = result.expect("get_setting for wrong_user/test_key failed");
        assert_eq!(value, None);

        // Test other methods to ensure macro generates them correctly
        let result =
            SettingsStore::delete_setting(&store, "test_user".into(), "test_key".into()).await;
        let deleted = result.expect("delete_setting for test_user/test_key failed");
        assert!(deleted);

        let result = SettingsStore::has_settings(&store, "test_user".into()).await;
        let has = result.expect("has_settings for test_user failed");
        assert!(!has);
    }
}
