//! Compatibility re-exports for the historical `crate::db::settings` path.
//!
//! The canonical settings traits and newtypes now live under the shared
//! database trait surface and are re-exported from `crate::db`. This module
//! preserves the older `crate::db::settings::{...}` import path for downstream
//! callers without maintaining a second copy of the API.

pub use crate::db::{NativeSettingsStore, SettingKey, SettingsStore, UserId};

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use super::*;
    use crate::error::DatabaseError;
    use crate::history::SettingRow;

    /// Asserts the standard string-newtype contract for a type that implements
    /// `From<&str>`, `Clone`, `PartialEq`, `Eq`, `Hash`, and `as_str() -> &str`.
    macro_rules! assert_string_newtype_contract {
        ($Type:ty, $primary:expr, $other:expr) => {{
            let value = <$Type>::from($primary);
            assert_eq!(value.as_str(), $primary);

            let cloned = value.clone();
            assert_eq!(cloned.as_str(), $primary);
            assert_eq!(value, cloned);

            let other = <$Type>::from($other);
            assert_ne!(value, other);

            let mut set = HashSet::new();
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
    async fn test_settings_forwarders_via_compat_module() {
        let store = DummySettingsStore;

        let result = SettingsStore::get_setting(
            &store,
            UserId::from("test_user"),
            SettingKey::from("test_key"),
        )
        .await;
        let value = result.expect("get_setting for test_user/test_key failed");
        assert_eq!(value, Some(serde_json::json!("test_value")));

        let result = SettingsStore::get_setting(
            &store,
            UserId::from("wrong_user"),
            SettingKey::from("test_key"),
        )
        .await;
        let value = result.expect("get_setting for wrong_user/test_key failed");
        assert_eq!(value, None);

        let result = SettingsStore::delete_setting(
            &store,
            UserId::from("test_user"),
            SettingKey::from("test_key"),
        )
        .await;
        let deleted = result.expect("delete_setting for test_user/test_key failed");
        assert!(deleted);

        let result = SettingsStore::has_settings(&store, UserId::from("test_user")).await;
        let has = result.expect("has_settings for test_user failed");
        assert!(!has);
    }
}
