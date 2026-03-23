//! Configuration loading abstraction for hot-reload.

use std::sync::Arc;

use async_trait::async_trait;

use crate::config::Config;
use crate::db::SettingsStore;
use crate::error::ConfigError;

/// Trait for loading configuration from various sources.
///
/// Implementations may read from the database, environment variables,
/// or other config sources.
#[async_trait]
pub trait ConfigLoader: Send + Sync {
    /// Load configuration.
    async fn load(&self) -> Result<Config, ConfigError>;
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

#[async_trait]
impl ConfigLoader for DbConfigLoader {
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

#[async_trait]
impl ConfigLoader for EnvConfigLoader {
    async fn load(&self) -> Result<Config, ConfigError> {
        Config::from_env().await
    }
}
