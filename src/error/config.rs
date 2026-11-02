//! Configuration-related error types.

use std::sync::Arc;

/// Configuration-related errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    /// A required environment variable is unset; the string is its name.
    #[error("Missing required environment variable: {0}")]
    MissingEnvVar(String),

    /// A required setting has no value in any source.
    #[error("Missing required configuration: {key}. {hint}")]
    MissingRequired {
        /// The configuration key that is missing.
        key: String,
        /// Advice on how to supply the missing value.
        hint: String,
    },

    /// A setting is present but its value is not acceptable.
    #[error("Invalid configuration value for {key}: {message}")]
    InvalidValue {
        /// The configuration key with the bad value.
        key: String,
        /// Why the value was rejected.
        message: String,
    },

    /// A configuration value could not be parsed into its expected type.
    #[error("Failed to parse configuration: {0}")]
    ParseError(String),

    /// Reading configuration from disk failed; shared so the error stays `Clone`.
    #[error("IO error: {0}")]
    Io(#[from] Arc<std::io::Error>),
}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(Arc::new(err))
    }
}

impl From<crate::settings::SettingsError> for ConfigError {
    /// Report a settings-file failure at the configuration boundary.
    ///
    /// [`crate::settings::Settings::load_from`] already treats a missing file
    /// as "use defaults", so any error here means the persisted configuration
    /// exists but could not be turned into settings.
    fn from(err: crate::settings::SettingsError) -> Self {
        Self::ParseError(err.to_string())
    }
}
