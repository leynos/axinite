//! Settings-related SettingsStore implementation for LibSqlBackend.

use std::collections::HashMap;

use libsql::params;

use super::{LibSqlBackend, fmt_ts, get_json, get_text, get_ts};
use crate::db::{NativeSettingsStore, SettingKey, UserId};
use crate::error::DatabaseError;
use crate::history::SettingRow;

use chrono::Utc;

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
        let conn = self.connect().await?;
        let now = fmt_ts(&Utc::now());
        conn.execute(
            r#"
                INSERT INTO settings (user_id, key, value, updated_at)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT (user_id, key) DO UPDATE SET
                    value = excluded.value,
                    updated_at = ?4
                "#,
            params![user_id.as_str(), key.as_str(), value.to_string(), now],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(())
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
}
