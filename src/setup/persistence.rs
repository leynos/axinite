//! Default settings persistence helper for setup wizard.
//!
//! Encapsulates all database interactions for the default user and session tokens,
//! keeping hard-coded identifiers and DB logic out of the orchestration code.

use crate::db::{Database, SettingKey, UserId};
use crate::error::DatabaseError;
use crate::settings::Settings;
use std::collections::HashMap;
use std::sync::Arc;

/// Default user ID for single-user setup wizard scenarios.
const DEFAULT_USER_ID: &str = "default";

/// Setting key for the NearAI session token.
const NEARAI_SESSION_TOKEN_KEY: &str = "nearai.session_token";

/// Helper for persisting default user settings during setup wizard.
pub struct DefaultSettingsPersistence {
    backend: Arc<dyn Database>,
}

impl DefaultSettingsPersistence {
    /// Create a new persistence helper for the given database backend.
    pub fn new(backend: Arc<dyn Database>) -> Self {
        Self { backend }
    }

    /// Save all settings for the default user to the database.
    ///
    /// Upserts each setting individually via `backend.set_setting` to merge
    /// defaults without deleting existing custom settings. This preserves
    /// user customisations while ensuring all required defaults are present.
    ///
    /// Note: Writes are incremental and partial commits may occur if a later
    /// `set_setting` fails. Callers that require atomicity should use a
    /// backend-level batch/transaction method instead.
    pub async fn save_default_settings(&self, settings: &Settings) -> Result<(), DatabaseError> {
        let user_id = UserId::from(DEFAULT_USER_ID);
        for (key, value) in settings.to_db_map() {
            self.backend
                .set_setting(user_id.clone(), SettingKey::from(key), &value)
                .await?;
        }
        Ok(())
    }

    /// Save the session token for the default user.
    pub async fn save_session_token(&self, value: &serde_json::Value) -> Result<(), DatabaseError> {
        self.backend
            .set_setting(
                UserId::from(DEFAULT_USER_ID),
                SettingKey::from(NEARAI_SESSION_TOKEN_KEY),
                value,
            )
            .await
    }

    /// Get all settings as a raw HashMap for the default user.
    pub async fn get_all_settings_map(
        &self,
    ) -> Result<HashMap<String, serde_json::Value>, DatabaseError> {
        self.backend
            .get_all_settings(UserId::from(DEFAULT_USER_ID))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[tokio::test]
    async fn save_default_settings_preserves_existing_session_token() {
        // Create a temporary file-based libSQL backend for testing
        // (in-memory databases don't persist across connections)
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let db_path = temp_dir.path().join("test.db");
        let backend: Arc<dyn Database> = Arc::new(
            crate::db::libsql::LibSqlBackend::new_local(&db_path)
                .await
                .expect("failed to create backend"),
        );

        // Run migrations to create the settings table
        backend
            .run_migrations()
            .await
            .expect("failed to run migrations");

        let persistence = DefaultSettingsPersistence::new(backend.clone());

        // Seed an existing session token
        let existing_token = serde_json::json!("test-session-token-123");
        persistence
            .save_session_token(&existing_token)
            .await
            .expect("failed to save session token");

        // Create default settings (without the session token)
        let settings = Settings::default();

        // Save default settings
        persistence
            .save_default_settings(&settings)
            .await
            .expect("failed to save default settings");

        // Read back all settings
        let stored_settings = persistence
            .get_all_settings_map()
            .await
            .expect("failed to get settings");

        // Verify the session token is still present and unchanged
        assert_eq!(
            stored_settings.get(NEARAI_SESSION_TOKEN_KEY),
            Some(&existing_token),
            "existing session token should be preserved after save_default_settings"
        );

        // Also verify some default setting was saved
        assert!(
            stored_settings.len() > 1,
            "default settings should also be present alongside the token"
        );
    }
}
