//! Configuration loading abstraction for hot-reload.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::config::Config;
use crate::db::SettingsStore;
use crate::error::ConfigError;

/// Boxed future used at the dyn config-loader boundary.
pub type ConfigLoaderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Config, ConfigError>> + Send + 'a>>;

/// Trait for loading configuration from various sources.
///
/// Implementations may read from the database, environment variables,
/// or other config sources.
pub trait ConfigLoader: Send + Sync {
    /// Load configuration.
    fn load<'a>(&'a self) -> ConfigLoaderFuture<'a>;
}

/// Native async sibling trait for concrete config-loader implementations.
pub trait NativeConfigLoader: Send + Sync {
    /// See [`ConfigLoader::load`].
    fn load(&self) -> impl Future<Output = Result<Config, ConfigError>> + Send + '_;
}

impl<T> ConfigLoader for T
where
    T: NativeConfigLoader + Send + Sync,
{
    fn load<'a>(&'a self) -> ConfigLoaderFuture<'a> {
        Box::pin(NativeConfigLoader::load(self))
    }
}

/// Config loader that reads from the database.
pub struct DbConfigLoader {
    settings_store: Arc<dyn SettingsStore>,
    user_id: String,
}

impl DbConfigLoader {
    /// Create a new database-backed config loader.
    ///
    /// `settings_store` — shared settings store; the Arc is cloned but the
    /// underlying store must live for the lifetime of the loader.
    /// `user_id` — identifier of the user whose configuration will be loaded.
    pub fn new(settings_store: Arc<dyn SettingsStore>, user_id: String) -> Self {
        Self {
            settings_store,
            user_id,
        }
    }
}

impl NativeConfigLoader for DbConfigLoader {
    async fn load(&self) -> Result<Config, ConfigError> {
        Config::from_db(self.settings_store.as_ref(), &self.user_id).await
    }
}

/// Config loader that reads from environment variables.
pub struct EnvConfigLoader;

impl EnvConfigLoader {
    /// Create a new environment-based config loader.
    ///
    /// Reads configuration from environment variables on each `load` call.
    /// Consider calling [`Config::from_env`] directly if hot-reload is not required.
    pub fn new() -> Self {
        Self
    }
}

impl Default for EnvConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeConfigLoader for EnvConfigLoader {
    async fn load(&self) -> Result<Config, ConfigError> {
        Config::from_env().await
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::*;
    use crate::db::settings::{NativeSettingsStore, SettingKey, UserId};
    use crate::error::DatabaseError;
    use crate::history::SettingRow;
    use crate::testing::test_utils::EnvVarsGuard;

    /// Mock SettingsStore for testing DbConfigLoader behavior.
    struct MockSettingsStore {
        settings: HashMap<String, serde_json::Value>,
    }

    impl MockSettingsStore {
        fn with_settings(settings: HashMap<String, serde_json::Value>) -> Self {
            Self { settings }
        }
    }

    impl NativeSettingsStore for MockSettingsStore {
        async fn get_setting(
            &self,
            _user_id: UserId,
            key: SettingKey,
        ) -> Result<Option<serde_json::Value>, DatabaseError> {
            Ok(self.settings.get(key.as_str()).cloned())
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
            Ok(false)
        }

        async fn list_settings(&self, _user_id: UserId) -> Result<Vec<SettingRow>, DatabaseError> {
            Ok(vec![])
        }

        async fn get_all_settings(
            &self,
            _user_id: UserId,
        ) -> Result<HashMap<String, serde_json::Value>, DatabaseError> {
            Ok(self.settings.clone())
        }

        async fn set_all_settings(
            &self,
            _user_id: UserId,
            _settings: &HashMap<String, serde_json::Value>,
        ) -> Result<(), DatabaseError> {
            Ok(())
        }

        async fn has_settings(&self, _user_id: UserId) -> Result<bool, DatabaseError> {
            Ok(!self.settings.is_empty())
        }
    }

    /// Test that EnvConfigLoader::new creates a zero-sized loader.
    #[test]
    fn env_config_loader_new_creates_loader() {
        let loader = EnvConfigLoader::new();
        // EnvConfigLoader is a ZST - no state, just behaviour
        assert_eq!(std::mem::size_of_val(&loader), 0);
    }

    /// Test that EnvConfigLoader implements Default via new().
    #[test]
    #[expect(
        clippy::default_constructed_unit_structs,
        reason = "EnvConfigLoader is unit-like and this test verifies new() and default() are equivalent"
    )]
    fn env_config_loader_default_uses_new() {
        let loader1 = EnvConfigLoader::new();
        let loader2 = EnvConfigLoader::default();

        // Both should be functionally equivalent (EnvConfigLoader has no state)
        assert_eq!(std::mem::size_of_val(&loader1), 0);
        assert_eq!(std::mem::size_of_val(&loader2), 0);
    }

    /// Test that DbConfigLoader correctly loads configuration from SettingsStore.
    ///
    /// Uses a mock SettingsStore to verify the loader fetches settings via get_setting
    /// and constructs a valid Config with the retrieved values.
    #[tokio::test]
    async fn db_config_loader_loads_config_from_store() {
        let mut env_guard = EnvVarsGuard::new(&["DATABASE_URL", "AGENT_NAME"]);
        env_guard.set("DATABASE_URL", "postgres://localhost/test");
        env_guard.remove("AGENT_NAME");

        // Create mock store with some test settings
        let mut settings = HashMap::new();
        settings.insert(
            "agent.name".to_string(),
            serde_json::json!("db-loader-agent"),
        );
        settings.insert("channels.http.port".to_string(), serde_json::json!(8080));

        let store = Arc::new(MockSettingsStore::with_settings(settings));
        let loader = DbConfigLoader::new(store, "test_user".to_string());

        // Call load() and verify it returns a valid Config
        let config = NativeConfigLoader::load(&loader)
            .await
            .expect("load should succeed");

        assert_eq!(
            config.agent.name, "db-loader-agent",
            "DbConfigLoader should preserve the seeded agent.name setting"
        );
    }
}
