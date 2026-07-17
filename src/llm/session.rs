//! Session management for NEAR AI authentication.
//!
//! Handles session token persistence, expiration detection, and renewal via
//! OAuth flow. Tokens are stored in `~/.axinite/session.json` and refreshed
//! automatically when expired.

mod oauth;
mod persistence;

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use reqwest::Client;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::llm::error::LlmError;

/// Session data persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub session_token: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub auth_provider: Option<String>,
}

/// Configuration for session management.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Base URL for auth endpoints (e.g., https://private.near.ai).
    pub auth_base_url: String,
    /// Path to session file (e.g., ~/.axinite/session.json).
    pub session_path: PathBuf,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            auth_base_url: "https://private.near.ai".to_string(),
            // Real path is set by LlmConfig::resolve() via config/llm.rs.
            // This default is only used in tests.
            session_path: PathBuf::from("session.json"),
        }
    }
}

/// Manages NEAR AI session tokens with persistence and automatic renewal.
pub struct SessionManager {
    config: SessionConfig,
    client: Client,
    /// Current token in memory.
    token: RwLock<Option<SecretString>>,
    /// Optional in-memory API key for NEAR AI Cloud authentication.
    api_key: RwLock<Option<SecretString>>,
    /// Prevents thundering herd during concurrent 401s.
    renewal_lock: Mutex<()>,
    /// Optional database store for persisting session to the settings table.
    store: RwLock<Option<Arc<dyn crate::db::Database>>>,
    /// User ID for DB settings (default: "default").
    user_id: RwLock<String>,
}

impl SessionManager {
    /// Create a new session manager and load any existing token from disk.
    pub fn new(config: SessionConfig) -> Self {
        let manager = Self {
            config,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
            token: RwLock::new(None),
            api_key: RwLock::new(None),
            renewal_lock: Mutex::new(()),
            store: RwLock::new(None),
            user_id: RwLock::new("default".to_string()),
        };

        // Try to load existing session synchronously during construction
        if let Ok(data) = ambient_fs::read_to_string(&manager.config.session_path)
            && let Ok(session) = serde_json::from_str::<SessionData>(&data)
        {
            // We can't await here, so we use try_write
            if let Ok(mut guard) = manager.token.try_write() {
                *guard = Some(SecretString::from(session.session_token));
                tracing::info!(
                    "Loaded session token from {}",
                    manager.config.session_path.display()
                );
            }
        }

        manager
    }

    /// Create a session manager and load token asynchronously.
    pub async fn new_async(config: SessionConfig) -> Self {
        let manager = Self {
            config,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
            token: RwLock::new(None),
            api_key: RwLock::new(None),
            renewal_lock: Mutex::new(()),
            store: RwLock::new(None),
            user_id: RwLock::new("default".to_string()),
        };

        if let Err(e) = manager.load_session().await {
            tracing::debug!("No existing session found: {}", e);
        }

        manager
    }

    /// Attach a database store for persisting session tokens.
    ///
    /// When a store is attached, session tokens are saved to the `settings`
    /// table (key: `nearai.session_token`) in addition to the disk file.
    /// On load, DB is preferred over disk.
    pub async fn attach_store(&self, store: Arc<dyn crate::db::Database>, user_id: &str) {
        *self.store.write().await = Some(store);
        *self.user_id.write().await = user_id.to_string();

        // Try to load from DB (may have been saved by a previous run)
        if let Err(e) = self.load_session_from_db().await {
            tracing::debug!("No session in DB: {}", e);
        }
    }

    /// Get the current session token, returning an error if not authenticated.
    pub async fn get_token(&self) -> Result<SecretString, LlmError> {
        let guard = self.token.read().await;
        guard.clone().ok_or_else(|| LlmError::AuthFailed {
            provider: "nearai".to_string(),
        })
    }

    /// Check if we have a valid token (doesn't verify with server).
    pub async fn has_token(&self) -> bool {
        self.token.read().await.is_some()
    }

    /// Check if we have an in-memory NEAR AI Cloud API key.
    pub async fn has_api_key(&self) -> bool {
        self.api_key.read().await.is_some()
    }

    /// Ensure we have a valid session, triggering login flow if needed.
    ///
    /// If no token exists, triggers the OAuth login flow. If a token exists,
    /// validates it by making a test API call. If validation fails, triggers
    /// the login flow.
    pub async fn ensure_authenticated(&self) -> Result<(), LlmError> {
        if self.has_api_key().await {
            return Ok(());
        }

        if !self.has_token().await {
            // No token, need to authenticate
            return self.initiate_login().await;
        }

        // Token exists, validate it by calling /v1/users/me
        tracing::debug!("Validating session...");
        match self.validate_token().await {
            Ok(()) => {
                tracing::debug!("Session valid");
                Ok(())
            }
            Err(e) => {
                tracing::info!("Session expired or invalid: {}", e);
                self.initiate_login().await
            }
        }
    }

    /// Validate the current token by calling the /v1/users/me endpoint.
    async fn validate_token(&self) -> Result<(), LlmError> {
        use secrecy::ExposeSecret;

        let token = self.get_token().await?;
        let url = format!("{}/v1/users/me", self.config.auth_base_url);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token.expose_secret()))
            .send()
            .await
            .map_err(|e| LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: format!("Validation request failed: {}", e),
            })?;

        if response.status().is_success() {
            return Ok(());
        }

        if response.status().as_u16() == 401 {
            return Err(LlmError::SessionExpired {
                provider: "nearai".to_string(),
            });
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let preview = crate::agent::truncate_for_preview(&body, 200);
        Err(LlmError::SessionRenewalFailed {
            provider: "nearai".to_string(),
            reason: format!("Validation failed: HTTP {status}: {preview}"),
        })
    }

    /// Handle an authentication failure (401 response).
    ///
    /// Triggers the OAuth login flow to get a new session token.
    pub async fn handle_auth_failure(&self) -> Result<(), LlmError> {
        // Acquire renewal lock to prevent thundering herd
        let _guard = self.renewal_lock.lock().await;

        tracing::info!("Session expired or invalid, re-authenticating...");
        self.initiate_login().await
    }

    /// Start the login flow.
    ///
    /// Shows the auth method menu FIRST (before binding any listener), so
    /// that the API-key path can skip network binding entirely. This is
    /// important for remote/headless servers where `127.0.0.1` is
    /// unreachable from the user's browser.
    ///
    /// For OAuth paths (GitHub, Google):
    /// 1. Bind the callback listener
    /// 2. Print the auth URL and attempt to open browser
    /// 3. Wait for OAuth callback with session token
    /// 4. Save and return the token
    ///
    /// For NEAR AI Cloud API key:
    /// 1. Prompt user for API key from cloud.near.ai
    /// 2. Store the key in the in-memory session manager for the current run
    /// 3. No session token saved (different auth model)
    async fn initiate_login(&self) -> Result<(), LlmError> {
        oauth::initiate_login_flow(self).await
    }

    /// NEAR AI Cloud API key entry flow.
    ///
    /// Prompts the user to enter a NEAR AI Cloud API key from
    /// cloud.near.ai. The key is stored in the session manager for the
    /// current run; onboarding persists it to encrypted secrets storage.
    /// No session token is saved and no `/v1/users/me` validation is
    /// performed (different auth model).
    async fn api_key_login(&self) -> Result<(), LlmError> {
        oauth::api_key_flow(self).await
    }

    /// Set token directly (useful for testing or migration from env var).
    pub async fn set_token(&self, token: SecretString) {
        let mut guard = self.token.write().await;
        *guard = Some(token);
    }

    /// Set an in-memory NEAR AI Cloud API key.
    pub async fn set_api_key(&self, api_key: SecretString) {
        let mut guard = self.api_key.write().await;
        *guard = Some(api_key);
    }

    /// Get the current in-memory NEAR AI Cloud API key, if one exists.
    pub async fn get_api_key(&self) -> Option<SecretString> {
        self.api_key.read().await.clone()
    }
}

/// Create a session manager from a config, loading env var if present.
///
/// When `NEARAI_SESSION_TOKEN` is set, it takes precedence over file-based
/// tokens. This supports hosting providers that inject the token via env var.
pub async fn create_session_manager(config: SessionConfig) -> Arc<SessionManager> {
    let manager = SessionManager::new_async(config).await;

    // NEARAI_SESSION_TOKEN env var always takes precedence over file-based
    // tokens. Hosting providers set this env var and expect it to be used
    // directly — no file persistence needed.
    if let Ok(token) = std::env::var("NEARAI_SESSION_TOKEN")
        && !token.is_empty()
    {
        tracing::info!("Using session token from NEARAI_SESSION_TOKEN env var");
        manager.set_token(SecretString::from(token)).await;
    }

    Arc::new(manager)
}

#[cfg(test)]
mod tests;
