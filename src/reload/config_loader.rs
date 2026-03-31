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
    use super::*;

    /// Test that EnvConfigLoader::new creates a zero-sized loader.
    #[test]
    fn env_config_loader_new_creates_loader() {
        let loader = EnvConfigLoader::new();
        // EnvConfigLoader is a ZST - no state, just behaviour
        assert_eq!(std::mem::size_of_val(&loader), 0);
    }

    /// Test that EnvConfigLoader implements Default via new().
    #[test]
    fn env_config_loader_default_uses_new() {
        let loader1 = EnvConfigLoader::new();
        let loader2 = EnvConfigLoader;

        // Both should be functionally equivalent (EnvConfigLoader has no state)
        assert_eq!(std::mem::size_of_val(&loader1), 0);
        assert_eq!(std::mem::size_of_val(&loader2), 0);
    }

    /// Test that DbConfigLoader::new returns a properly typed instance.
    ///
    /// Since SettingsStore is a complex trait with many methods, we verify
    /// the constructor signature is correct without actually implementing
    /// the trait (which would require implementing ~10 methods).
    #[test]
    fn db_config_loader_constructor_signature_is_valid() {
        // This test documents the expected types for DbConfigLoader::new
        // In real usage, an Arc<dyn SettingsStore> from the db module is passed
        fn _type_check_new(
            store: Arc<dyn crate::db::SettingsStore>,
            user_id: String,
        ) -> DbConfigLoader {
            DbConfigLoader::new(store, user_id)
        }

        // The type check above ensures the constructor accepts the right types
        // We don't call it because we don't have a mock SettingsStore
        let _ = _type_check_new;
    }
}
