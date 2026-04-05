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
    pub async fn save_default_settings(&self, settings: &Settings) -> Result<(), DatabaseError> {
        self.backend
            .set_all_settings(UserId::from(DEFAULT_USER_ID), &settings.to_db_map())
            .await
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
