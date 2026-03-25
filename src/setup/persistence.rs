//! Default settings persistence helper for setup wizard.
//!
//! Encapsulates all database interactions for the default user and session tokens,
//! keeping hard-coded identifiers and DB logic out of the orchestration code.
//!
//! This module is ready for integration but not yet wired into SetupWizard.

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

    /// Load all settings for the default user from the database.
    pub async fn load_default_settings(&self) -> Result<Settings, DatabaseError> {
        let map = self
            .backend
            .get_all_settings(UserId::from(DEFAULT_USER_ID))
            .await?;
        Ok(Settings::from_db_map(&map))
    }

    /// Save all settings for the default user to the database.
    pub async fn save_default_settings(&self, settings: &Settings) -> Result<(), DatabaseError> {
        let db_map = settings.to_db_map();
        self.backend
            .set_all_settings(UserId::from(DEFAULT_USER_ID), &db_map)
            .await
    }

    /// Load the session token for the default user.
    pub async fn load_session_token(&self) -> Result<Option<serde_json::Value>, DatabaseError> {
        self.backend
            .get_setting(
                UserId::from(DEFAULT_USER_ID),
                SettingKey::from(NEARAI_SESSION_TOKEN_KEY),
            )
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

    /// Check if default user has any settings in the database.
    pub async fn has_default_settings(&self) -> Result<bool, DatabaseError> {
        self.backend
            .has_settings(UserId::from(DEFAULT_USER_ID))
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
