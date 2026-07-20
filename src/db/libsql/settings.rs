//! Settings-related SettingsStore implementation for LibSqlBackend.

use std::collections::HashMap;

use libsql::params;

use super::{LibSqlBackend, fmt_ts, get_i64, get_json, get_text, get_ts};
use crate::db::{NativeSettingsStore, SettingKey, UserId};
use crate::error::DatabaseError;
use crate::history::SettingRow;

use chrono::Utc;

impl LibSqlBackend {
    /// Run a timestamped two-key upsert statement on a fresh connection.
    ///
    /// Shared by the settings and feature-flag writers so the timestamp,
    /// connect, execute, and error plumbing live in one place; `build_params`
    /// receives the formatted current timestamp for the `?4` slot.
    async fn execute_timestamped_upsert<P: libsql::params::IntoParams>(
        &self,
        sql: &str,
        build_params: impl FnOnce(String) -> P,
    ) -> Result<(), DatabaseError> {
        let now = fmt_ts(&Utc::now());
        let conn = self.connect().await?;
        conn.execute(sql, build_params(now))
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(())
    }
}

impl NativeSettingsStore for LibSqlBackend {
    async fn get_setting(
        &self,
        user_id: UserId,
        key: SettingKey,
    ) -> Result<Option<serde_json::Value>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT value FROM settings WHERE user_id = ?1 AND key = ?2",
                params![user_id.as_str(), key.as_str()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            Some(row) => Ok(Some(get_json(&row, 0))),
            None => Ok(None),
        }
    }

    async fn get_setting_full(
        &self,
        user_id: UserId,
        key: SettingKey,
    ) -> Result<Option<SettingRow>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT key, value, updated_at FROM settings WHERE user_id = ?1 AND key = ?2",
                params![user_id.as_str(), key.as_str()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            Some(row) => Ok(Some(SettingRow {
                key: SettingKey::from(get_text(&row, 0)),
                value: get_json(&row, 1),
                updated_at: get_ts(&row, 2),
            })),
            None => Ok(None),
        }
    }

    async fn set_setting(
        &self,
        user_id: UserId,
        key: SettingKey,
        value: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        self.execute_timestamped_upsert(
            r#"
                INSERT INTO settings (user_id, key, value, updated_at)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT (user_id, key) DO UPDATE SET
                    value = excluded.value,
                    updated_at = ?4
                "#,
            |now| params![user_id.as_str(), key.as_str(), value.to_string(), now],
        )
        .await
    }

    async fn delete_setting(
        &self,
        user_id: UserId,
        key: SettingKey,
    ) -> Result<bool, DatabaseError> {
        let conn = self.connect().await?;
        let count = conn
            .execute(
                "DELETE FROM settings WHERE user_id = ?1 AND key = ?2",
                params![user_id.as_str(), key.as_str()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(count > 0)
    }

    async fn list_settings(&self, user_id: UserId) -> Result<Vec<SettingRow>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT key, value, updated_at FROM settings WHERE user_id = ?1 ORDER BY key",
                params![user_id.as_str()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut settings = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            settings.push(SettingRow {
                key: SettingKey::from(get_text(&row, 0)),
                value: get_json(&row, 1),
                updated_at: get_ts(&row, 2),
            });
        }
        Ok(settings)
    }

    async fn get_all_settings(
        &self,
        user_id: UserId,
    ) -> Result<HashMap<String, serde_json::Value>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT key, value FROM settings WHERE user_id = ?1",
                params![user_id.as_str()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut map = HashMap::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            map.insert(get_text(&row, 0), get_json(&row, 1));
        }
        Ok(map)
    }

    async fn set_all_settings(
        &self,
        user_id: UserId,
        settings: &HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        let conn = self.connect().await?;
        let now = fmt_ts(&Utc::now());
        conn.execute("BEGIN", ())
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let delete_result = if settings.is_empty() {
            conn.execute(
                "DELETE FROM settings WHERE user_id = ?1",
                params![user_id.as_str()],
            )
            .await
        } else {
            let placeholders = (0..settings.len())
                .map(|index| format!("?{}", index + 2))
                .collect::<Vec<_>>()
                .join(", ");
            let sql =
                format!("DELETE FROM settings WHERE user_id = ?1 AND key NOT IN ({placeholders})");
            let mut values = Vec::with_capacity(settings.len() + 1);
            values.push(user_id.as_str().into());
            values.extend(settings.keys().map(|key| key.as_str().into()));
            conn.execute(&sql, libsql::params::Params::Positional(values))
                .await
        };
        if let Err(e) = delete_result {
            let _ = conn.execute("ROLLBACK", ()).await;
            return Err(DatabaseError::Query(e.to_string()));
        }

        for (key, value) in settings {
            if let Err(e) = conn
                .execute(
                    r#"
                    INSERT INTO settings (user_id, key, value, updated_at)
                    VALUES (?1, ?2, ?3, ?4)
                    ON CONFLICT (user_id, key) DO UPDATE SET
                        value = excluded.value,
                        updated_at = ?4
                    "#,
                    params![
                        user_id.as_str(),
                        key.as_str(),
                        value.to_string(),
                        now.as_str()
                    ],
                )
                .await
            {
                let _ = conn.execute("ROLLBACK", ()).await;
                return Err(DatabaseError::Query(e.to_string()));
            }
        }

        conn.execute("COMMIT", ())
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(())
    }

    async fn has_settings(&self, user_id: UserId) -> Result<bool, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT EXISTS(SELECT 1 FROM settings WHERE user_id = ?1)",
                params![user_id.as_str()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            Some(row) => {
                let exists: i64 = row
                    .get(0)
                    .map_err(|e| DatabaseError::Query(e.to_string()))?;
                Ok(exists != 0)
            }
            None => Ok(false),
        }
    }

    async fn list_deployment_flags(
        &self,
        deployment_id: &str,
    ) -> Result<Vec<(String, bool)>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT flag_name, enabled FROM feature_flag_overrides \
                 WHERE deployment_id = ?1 ORDER BY flag_name",
                params![deployment_id],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut flags = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            // Booleans store as INTEGER (0/1) in libSQL; read via get_i64.
            flags.push((get_text(&row, 0), get_i64(&row, 1) != 0));
        }
        Ok(flags)
    }

    async fn set_deployment_flag(
        &self,
        deployment_id: &str,
        flag_name: &str,
        enabled: bool,
    ) -> Result<(), DatabaseError> {
        self.execute_timestamped_upsert(
            r#"
                INSERT INTO feature_flag_overrides (deployment_id, flag_name, enabled, updated_at)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT (deployment_id, flag_name) DO UPDATE SET
                    enabled = excluded.enabled,
                    updated_at = ?4
                "#,
            |now| params![deployment_id, flag_name, i64::from(enabled), now],
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    //! Round-trip tests for deployment-scoped feature-flag persistence.

    use crate::db::libsql::LibSqlBackend;
    use crate::db::{Database, NativeSettingsStore};

    #[tokio::test]
    async fn deployment_flag_round_trip_upserts_and_isolates_deployments() {
        let backend = LibSqlBackend::new_memory().await.unwrap();
        backend.run_migrations().await.unwrap();

        // No overrides initially.
        assert!(
            backend
                .list_deployment_flags("production")
                .await
                .unwrap()
                .is_empty()
        );

        backend
            .set_deployment_flag("production", "panel_logs", false)
            .await
            .unwrap();
        backend
            .set_deployment_flag("production", "route_chat", true)
            .await
            .unwrap();
        // Upsert: writing the same key again replaces the stored value.
        backend
            .set_deployment_flag("production", "panel_logs", true)
            .await
            .unwrap();

        // ORDER BY flag_name -> panel_logs before route_chat.
        let flags = backend.list_deployment_flags("production").await.unwrap();
        assert_eq!(
            flags,
            vec![
                ("panel_logs".to_string(), true),
                ("route_chat".to_string(), true),
            ]
        );

        // A different deployment is isolated.
        assert!(
            backend
                .list_deployment_flags("staging")
                .await
                .unwrap()
                .is_empty()
        );
    }
}
