//! Session persistence: disk file and database settings storage.

use chrono::Utc;
use secrecy::SecretString;

use crate::llm::error::LlmError;

use super::{SessionData, SessionManager};

impl SessionManager {
    /// Save session data to disk and (if available) to the database.
    pub(super) async fn save_session(
        &self,
        token: &str,
        auth_provider: Option<&str>,
    ) -> Result<(), LlmError> {
        let session = SessionData {
            session_token: token.to_string(),
            created_at: Utc::now(),
            auth_provider: auth_provider.map(String::from),
        };

        // Save to disk (always, as bootstrap fallback)
        self.save_session_to_disk(&session).await?;

        // Also save to DB if a store is attached
        self.save_session_to_db(&session, token).await;

        Ok(())
    }

    /// Write the session file with restrictive permissions.
    async fn save_session_to_disk(&self, session: &SessionData) -> Result<(), LlmError> {
        if let Some(parent) = self.config.session_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                LlmError::Io(std::io::Error::new(
                    e.kind(),
                    format!("Failed to create session directory: {}", e),
                ))
            })?;
        }

        let json =
            serde_json::to_string_pretty(session).map_err(|e| LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: format!("Failed to serialize session: {}", e),
            })?;

        tokio::fs::write(&self.config.session_path, json)
            .await
            .map_err(|e| {
                LlmError::Io(std::io::Error::new(
                    e.kind(),
                    format!(
                        "Failed to write session file {}: {}",
                        self.config.session_path.display(),
                        e
                    ),
                ))
            })?;

        self.restrict_session_file_permissions().await?;

        tracing::debug!("Session saved to {}", self.config.session_path.display());
        Ok(())
    }

    /// Restrict the session file to owner read/write — it contains a secret
    /// token. No-op on non-Unix platforms.
    async fn restrict_session_file_permissions(&self) -> Result<(), LlmError> {
        #[cfg(unix)]
        {
            let perms = ambient_fs::Permissions::from_mode(0o600);
            tokio::fs::set_permissions(&self.config.session_path, perms.into_std())
                .await
                .map_err(|e| {
                    LlmError::Io(std::io::Error::new(
                        e.kind(),
                        format!(
                            "Failed to set permissions on {}: {}",
                            self.config.session_path.display(),
                            e
                        ),
                    ))
                })?;
        }
        Ok(())
    }

    /// Best-effort save of the session to DB settings (warns on failure).
    async fn save_session_to_db(&self, session: &SessionData, token: &str) {
        let Some(ref store) = *self.store.read().await else {
            return;
        };
        let user_id = self.user_id.read().await.clone();
        let session_json =
            serde_json::to_value(session).unwrap_or(serde_json::Value::String(token.to_string()));
        if let Err(e) = store
            .set_setting(
                crate::db::UserId::from(user_id.as_str()),
                crate::db::SettingKey::from("nearai.session_token"),
                &session_json,
            )
            .await
        {
            tracing::warn!("Failed to save session to DB: {}", e);
        } else {
            tracing::debug!("Session also saved to DB settings");
        }
    }

    /// Try to load session from the database.
    pub(super) async fn load_session_from_db(&self) -> Result<(), LlmError> {
        let store_guard = self.store.read().await;
        let store = store_guard
            .as_ref()
            .ok_or_else(|| LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: "No DB store attached".to_string(),
            })?;

        let user_id = self.user_id.read().await.clone();
        let value = if let Some(value) = store
            .get_setting(
                crate::db::UserId::from(user_id.as_str()),
                crate::db::SettingKey::from("nearai.session_token"),
            )
            .await
            .map_err(|e| LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: format!("DB query failed: {}", e),
            })? {
            value
        } else {
            // Try the legacy key. Only warn if it actually exists (real
            // backwards-compat migration). When neither key is present
            // (fresh install), just return the "No session in DB" error.
            let legacy = store
                .get_setting(
                    crate::db::UserId::from(user_id.as_str()),
                    crate::db::SettingKey::from("nearai.session"),
                )
                .await
                .map_err(|e| LlmError::SessionRenewalFailed {
                    provider: "nearai".to_string(),
                    reason: format!("DB query failed: {}", e),
                })?;
            match legacy {
                Some(value) => {
                    tracing::warn!(
                        "nearai.session_token missing; falling back to legacy nearai.session for backwards compatibility"
                    );
                    value
                }
                None => {
                    return Err(LlmError::SessionRenewalFailed {
                        provider: "nearai".to_string(),
                        reason: "No session in DB".to_string(),
                    });
                }
            }
        };

        let session: SessionData =
            serde_json::from_value(value).map_err(|e| LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: format!("Failed to parse DB session: {}", e),
            })?;

        let mut guard = self.token.write().await;
        *guard = Some(SecretString::from(session.session_token));
        tracing::info!("Loaded session from DB settings");

        Ok(())
    }

    /// Load session data from disk.
    pub(super) async fn load_session(&self) -> Result<(), LlmError> {
        let data = tokio::fs::read_to_string(&self.config.session_path)
            .await
            .map_err(|e| {
                LlmError::Io(std::io::Error::new(
                    e.kind(),
                    format!(
                        "Failed to read session file {}: {}",
                        self.config.session_path.display(),
                        e
                    ),
                ))
            })?;

        let session: SessionData =
            serde_json::from_str(&data).map_err(|e| LlmError::SessionRenewalFailed {
                provider: "nearai".to_string(),
                reason: format!("Failed to parse session file: {}", e),
            })?;

        {
            let mut guard = self.token.write().await;
            *guard = Some(SecretString::from(session.session_token));
        }

        tracing::info!(
            "Loaded session from {} (created: {})",
            self.config.session_path.display(),
            session.created_at
        );

        Ok(())
    }
}
