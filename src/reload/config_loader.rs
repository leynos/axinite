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
