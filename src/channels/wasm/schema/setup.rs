//! Setup wizard schema for WASM channel capabilities files.
//!
//! Declares the secrets a channel requires during setup, optional
//! validation endpoints, and auto-generation settings.

use serde::{Deserialize, Serialize};

/// Setup configuration schema.
///
/// Allows channels to declare their setup requirements for the wizard.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SetupSchema {
    /// Required secrets that must be configured during setup.
    #[serde(default)]
    pub required_secrets: Vec<SecretSetupSchema>,

    /// Optional validation endpoint to verify configuration.
    /// Placeholders like {secret_name} are replaced with actual values.
    #[serde(default)]
    pub validation_endpoint: Option<String>,

    /// User-facing URL where they can create/manage credentials.
    #[serde(default)]
    pub setup_url: Option<String>,
}

/// Configuration for a secret required during setup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretSetupSchema {
    /// Secret name in the secrets store (e.g., "telegram_bot_token").
    pub name: String,

    /// Prompt to show the user during setup.
    pub prompt: String,

    /// Optional regex for validation.
    #[serde(default)]
    pub validation: Option<String>,

    /// Whether this secret is optional.
    #[serde(default)]
    pub optional: bool,

    /// Auto-generate configuration if the user doesn't provide a value.
    #[serde(default)]
    pub auto_generate: Option<AutoGenerateSchema>,
}

/// Configuration for auto-generating a secret value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoGenerateSchema {
    /// Length of the generated value in bytes (will be hex-encoded).
    #[serde(default = "default_auto_generate_length")]
    pub length: usize,
}

fn default_auto_generate_length() -> usize {
    32
}
