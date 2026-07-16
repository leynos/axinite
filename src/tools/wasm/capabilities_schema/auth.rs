//! Authentication and setup schemas: OAuth configuration, manual token
//! entry instructions, validation endpoints, and required-secret setup.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Authentication setup schema.
///
/// Tools declare their auth requirements here. The agent uses this to provide
/// generic auth flows without needing service-specific code in the main codebase.
///
/// Supports two auth methods:
/// 1. **OAuth** - Browser-based login (preferred for user-facing services)
/// 2. **Manual** - Copy/paste token from provider's dashboard
///
/// # Example (OAuth)
///
/// ```json
/// {
///   "auth": {
///     "secret_name": "notion_api_token",
///     "display_name": "Notion",
///     "oauth": {
///       "authorization_url": "https://api.notion.com/v1/oauth/authorize",
///       "token_url": "https://api.notion.com/v1/oauth/token",
///       "client_id": "your-client-id",
///       "scopes": []
///     },
///     "env_var": "NOTION_TOKEN"
///   }
/// }
/// ```
///
/// # Example (Manual)
///
/// ```json
/// {
///   "auth": {
///     "secret_name": "openai_api_key",
///     "display_name": "OpenAI",
///     "instructions": "Get your API key from platform.openai.com/api-keys",
///     "setup_url": "https://platform.openai.com/api-keys",
///     "token_hint": "Starts with 'sk-'",
///     "env_var": "OPENAI_API_KEY"
///   }
/// }
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuthCapabilitySchema {
    /// Name of the secret to store (e.g., "notion_api_token").
    /// Must match the secret_name in credentials if HTTP capability is used.
    pub secret_name: String,

    /// Human-readable name for the service (e.g., "Notion", "Slack").
    #[serde(default)]
    pub display_name: Option<String>,

    /// OAuth configuration for browser-based login.
    /// If present, OAuth flow is used instead of manual token entry.
    #[serde(default)]
    pub oauth: Option<OAuthConfigSchema>,

    /// Instructions shown to the user for obtaining credentials (manual flow).
    /// Can include markdown formatting.
    #[serde(default)]
    pub instructions: Option<String>,

    /// URL to open for setting up credentials (manual flow).
    #[serde(default)]
    pub setup_url: Option<String>,

    /// Hint about expected token format (e.g., "Starts with 'sk-'").
    /// Used for validation feedback.
    #[serde(default)]
    pub token_hint: Option<String>,

    /// Environment variable to check before prompting.
    /// If this env var is set, its value is used automatically.
    #[serde(default)]
    pub env_var: Option<String>,

    /// Provider hint for organizing secrets (e.g., "notion", "openai").
    #[serde(default)]
    pub provider: Option<String>,

    /// Validation endpoint to check if the token works.
    /// Tool can specify an endpoint to call for validation.
    #[serde(default)]
    pub validation_endpoint: Option<ValidationEndpointSchema>,
}

/// OAuth 2.0 configuration for browser-based login.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OAuthConfigSchema {
    /// OAuth authorization URL (e.g., "https://api.notion.com/v1/oauth/authorize").
    pub authorization_url: String,

    /// OAuth token exchange URL (e.g., "https://api.notion.com/v1/oauth/token").
    pub token_url: String,

    /// OAuth client ID.
    /// Can be set here or via environment variable (see client_id_env).
    #[serde(default)]
    pub client_id: Option<String>,

    /// Environment variable containing the client ID.
    /// Checked if client_id is not set directly.
    #[serde(default)]
    pub client_id_env: Option<String>,

    /// OAuth client secret (optional, some providers don't require it with PKCE).
    /// Can be set here or via environment variable (see client_secret_env).
    #[serde(default)]
    pub client_secret: Option<String>,

    /// Environment variable containing the client secret.
    /// Checked if client_secret is not set directly.
    #[serde(default)]
    pub client_secret_env: Option<String>,

    /// OAuth scopes to request.
    #[serde(default)]
    pub scopes: Vec<String>,

    /// Use PKCE (Proof Key for Code Exchange). Defaults to true.
    /// Required for public clients (CLI tools).
    #[serde(default = "default_true")]
    pub use_pkce: bool,

    /// Additional parameters to include in the authorization URL.
    #[serde(default)]
    pub extra_params: std::collections::HashMap<String, String>,

    /// Field name in token response containing the access token.
    /// Defaults to "access_token".
    #[serde(default = "default_access_token_field")]
    pub access_token_field: String,
}

fn default_true() -> bool {
    true
}

fn default_access_token_field() -> String {
    "access_token".to_string()
}

/// Schema for token validation endpoint.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ValidationEndpointSchema {
    /// URL to call for validation (e.g., "https://api.notion.com/v1/users/me").
    pub url: String,

    /// HTTP method (defaults to GET).
    #[serde(default = "default_method")]
    pub method: String,

    /// Expected HTTP status code for success (defaults to 200).
    #[serde(default = "default_success_status")]
    pub success_status: u16,

    /// Additional headers to send with the validation request.
    /// Used for service-specific requirements (e.g., Notion-Version for Notion API).
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_success_status() -> u16 {
    200
}

/// Setup schema for WASM tools: secrets the user must provide via the UI.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolSetupSchema {
    /// Secrets the user must provide before the tool can be used.
    #[serde(default)]
    pub required_secrets: Vec<ToolSecretSetupSchema>,
}

/// A single secret required during tool setup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSecretSetupSchema {
    /// Secret name in the secrets store (e.g. "google_oauth_client_id").
    pub name: String,
    /// User-facing prompt (e.g. "Google OAuth Client ID").
    pub prompt: String,
    /// If true, the user may skip this secret.
    #[serde(default)]
    pub optional: bool,
}
