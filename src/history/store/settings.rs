//! Settings persistence helpers.

use chrono::{DateTime, Utc};

#[cfg(feature = "postgres")]
use super::Store;
use crate::db::SettingKey;
#[cfg(feature = "postgres")]
use crate::db::UserId;
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;

/// A single setting row from the database.
#[derive(Debug, Clone)]
pub struct SettingRow {
    /// Strongly typed key for the persisted setting row.
    pub key: SettingKey,
    /// JSON payload stored for the setting key.
    pub value: serde_json::Value,
    /// Timestamp of the most recent write for this row.
    pub updated_at: DateTime<Utc>,
}

#[cfg(feature = "postgres")]
impl Store {
    /// Get a single setting by key.
    pub async fn get_setting(
        &self,
        user_id: UserId,
        key: SettingKey,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_opt(
                "SELECT value FROM settings WHERE user_id = $1 AND key = $2",
                &[&user_id.as_str(), &key.as_str()],
            )
            .await?;
        Ok(row.map(|r| r.get("value")))
    }

    /// Get a single setting with full metadata.
    pub async fn get_setting_full(
        &self,
        user_id: UserId,
        key: SettingKey,
    ) -> Result<Option<SettingRow>, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_opt(
                "SELECT key, value, updated_at FROM settings WHERE user_id = $1 AND key = $2",
                &[&user_id.as_str(), &key.as_str()],
            )
            .await?;
        Ok(row.map(|r| SettingRow {
            key: SettingKey::from(r.get::<_, String>("key")),
            value: r.get("value"),
            updated_at: r.get("updated_at"),
        }))
    }

    /// Set a single setting (upsert).
    pub async fn set_setting(
        &self,
        user_id: UserId,
        key: SettingKey,
        value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        conn.execute(
            r#"
            INSERT INTO settings (user_id, key, value, updated_at)
            VALUES ($1, $2, $3, NOW())
            ON CONFLICT (user_id, key) DO UPDATE SET
                value = EXCLUDED.value,
                updated_at = NOW()
            "#,
            &[&user_id.as_str(), &key.as_str(), value],
        )
        .await?;
        Ok(())
    }

    /// Delete a single setting (reset to default).
    pub async fn delete_setting(
        &self,
        user_id: UserId,
        key: SettingKey,
    ) -> Result<bool, DatabaseError> {
        let conn = self.conn().await?;
        let count = conn
            .execute(
                "DELETE FROM settings WHERE user_id = $1 AND key = $2",
                &[&user_id.as_str(), &key.as_str()],
            )
            .await?;
        Ok(count > 0)
    }

    /// List all settings for a user (with metadata).
    pub async fn list_settings(&self, user_id: UserId) -> Result<Vec<SettingRow>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT key, value, updated_at FROM settings WHERE user_id = $1 ORDER BY key",
                &[&user_id.as_str()],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|r| SettingRow {
                key: SettingKey::from(r.get::<_, String>("key")),
                value: r.get("value"),
                updated_at: r.get("updated_at"),
            })
            .collect())
    }

    /// Get all settings as a flat key-value map.
    pub async fn get_all_settings(
        &self,
        user_id: UserId,
    ) -> Result<std::collections::HashMap<String, serde_json::Value>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT key, value FROM settings WHERE user_id = $1",
                &[&user_id.as_str()],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|r| {
                let key: String = r.get("key");
                let value: serde_json::Value = r.get("value");
                (key, value)
            })
            .collect())
    }

    /// Bulk-write settings (used for migration/import).
    pub async fn set_all_settings(
        &self,
        user_id: UserId,
        settings: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await?;

        if settings.is_empty() {
            tx.execute(
                "DELETE FROM settings WHERE user_id = $1",
                &[&user_id.as_str()],
            )
            .await?;
        } else {
            let keys: Vec<String> = settings.keys().cloned().collect();
            tx.execute(
                "DELETE FROM settings WHERE user_id = $1 AND NOT (key = ANY($2))",
                &[&user_id.as_str(), &keys],
            )
            .await?;
        }

        for (key, value) in settings {
            tx.execute(
                r#"
                INSERT INTO settings (user_id, key, value, updated_at)
                VALUES ($1, $2, $3, NOW())
                ON CONFLICT (user_id, key) DO UPDATE SET
                    value = EXCLUDED.value,
                    updated_at = NOW()
                "#,
                &[&user_id.as_str(), &key.as_str(), value],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// List deployment-scoped feature-flag overrides for a deployment.
    ///
    /// Feature flags (RFC 0009) are deployment-scoped, stored in the dedicated
    /// `feature_flag_overrides` table rather than the user-scoped `settings`
    /// table.
    pub async fn list_deployment_flags(
        &self,
        deployment_id: &str,
    ) -> Result<Vec<(String, bool)>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT flag_name, enabled FROM feature_flag_overrides \
                 WHERE deployment_id = $1 ORDER BY flag_name",
                &[&deployment_id],
            )
            .await?;
        Ok(rows
            .iter()
            .map(|r| {
                let name: String = r.get("flag_name");
                let enabled: bool = r.get("enabled");
                (name, enabled)
            })
            .collect())
    }

    /// Insert or replace one deployment-scoped feature-flag override (upsert).
    pub async fn set_deployment_flag(
        &self,
        deployment_id: &str,
        flag_name: &str,
        enabled: bool,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        conn.execute(
            r#"
            INSERT INTO feature_flag_overrides (deployment_id, flag_name, enabled, updated_at)
            VALUES ($1, $2, $3, NOW())
            ON CONFLICT (deployment_id, flag_name) DO UPDATE SET
                enabled = EXCLUDED.enabled,
                updated_at = NOW()
            "#,
            &[&deployment_id, &flag_name, &enabled],
        )
        .await?;
        Ok(())
    }

    /// Check if the settings table has any rows for a user.
    pub async fn has_settings(&self, user_id: UserId) -> Result<bool, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM settings WHERE user_id = $1)",
                &[&user_id.as_str()],
            )
            .await?;
        let exists: bool = row.get(0);
        Ok(exists)
    }
}
