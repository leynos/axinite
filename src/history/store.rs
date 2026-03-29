//! PostgreSQL store for persisting agent data.

use chrono::{DateTime, Utc};
#[cfg(feature = "postgres")]
use deadpool_postgres::{Config, Pool};
use rust_decimal::Decimal;
use uuid::Uuid;

#[cfg(feature = "postgres")]
use crate::config::DatabaseConfig;
#[cfg(feature = "postgres")]
use crate::context::{ActionRecord, JobContext, JobState};
#[cfg(feature = "postgres")]
use crate::db::{SettingKey, UserId};
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;
#[cfg(feature = "postgres")]
use crate::history::migrations::run_postgres_migrations;

/// Record for an LLM call to be persisted.
#[derive(Debug, Clone)]
pub struct LlmCallRecord<'a> {
    pub job_id: Option<Uuid>,
    pub conversation_id: Option<Uuid>,
    pub provider: &'a str,
    pub model: &'a str,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cost: Decimal,
    pub purpose: Option<&'a str>,
}

/// Database store for the agent.
#[cfg(feature = "postgres")]
pub struct Store {
    pool: Pool,
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
            key: r.get("key"),
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
                key: r.get("key"),
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
    ///
    /// Each entry is upserted individually within a single transaction.
    pub async fn set_all_settings(
        &self,
        user_id: UserId,
        settings: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await?;

        for (key, value) in settings {
            tx.execute(
                r#"
                INSERT INTO settings (user_id, key, value, updated_at)
                VALUES ($1, $2, $3, NOW())
                ON CONFLICT (user_id, key) DO UPDATE SET
                    value = EXCLUDED.value,
                    updated_at = NOW()
                "#,
                &[&user_id.as_str(), &key, value],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Check if the settings table has any rows for a user.
    pub async fn has_settings(&self, user_id: UserId) -> Result<bool, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_one(
                "SELECT COUNT(*) as cnt FROM settings WHERE user_id = $1",
                &[&user_id.as_str()],
            )
            .await?;
        let count: i64 = row.get("cnt");
        Ok(count > 0)
    }
}

// ==================== Sandbox Jobs ====================

/// Record for a sandbox container job, persisted in the `agent_jobs` table
/// with `source = 'sandbox'`.
#[derive(Debug, Clone)]
pub struct SandboxJobRecord {
    pub id: Uuid,
    pub task: String,
    pub status: String,
    pub user_id: String,
    pub project_dir: String,
    pub success: Option<bool>,
    pub failure_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    /// Serialized JSON of `Vec<CredentialGrant>` for restart support.
    /// Stored in the `description` column of `agent_jobs` (unused for sandbox jobs).
    pub credential_grants_json: String,
}

/// Summary of sandbox job counts grouped by status.
#[derive(Debug, Clone, Default)]
pub struct SandboxJobSummary {
    pub total: usize,
    pub creating: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub interrupted: usize,
}

/// Lightweight record for agent (non-sandbox) jobs, used by the web Jobs tab.
#[derive(Debug, Clone)]
pub struct AgentJobRecord {
    pub id: Uuid,
    pub title: String,
    pub status: String,
    pub user_id: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub failure_reason: Option<String>,
}

/// Summary counts for agent (non-sandbox) jobs.
#[derive(Debug, Clone, Default)]
pub struct AgentJobSummary {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub failed: usize,
    pub stuck: usize,
}

impl AgentJobSummary {
    /// Accumulate a status/count pair into the summary buckets.
    pub fn add_count(&mut self, status: &str, count: usize) {
        self.total += count;
        match status {
            "pending" => self.pending += count,
            "in_progress" => self.in_progress += count,
            "completed" | "submitted" | "accepted" => self.completed += count,
            "failed" | "cancelled" => self.failed += count,
            "stuck" => self.stuck += count,
            _ => {}
        }
    }
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
            key: r.get("key"),
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
                key: r.get("key"),
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
    ///
    /// Each entry is upserted individually within a single transaction.
    pub async fn set_all_settings(
        &self,
        user_id: UserId,
        settings: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await?;

        for (key, value) in settings {
            tx.execute(
                r#"
                INSERT INTO settings (user_id, key, value, updated_at)
                VALUES ($1, $2, $3, NOW())
                ON CONFLICT (user_id, key) DO UPDATE SET
                    value = EXCLUDED.value,
                    updated_at = NOW()
                "#,
                &[&user_id.as_str(), &key, value],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Check if the settings table has any rows for a user.
    pub async fn has_settings(&self, user_id: UserId) -> Result<bool, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_one(
                "SELECT COUNT(*) as cnt FROM settings WHERE user_id = $1",
                &[&user_id.as_str()],
            )
            .await?;
        let count: i64 = row.get("cnt");
        Ok(count > 0)
    }
}

// ==================== Job Events ====================

/// A persisted job streaming event (from worker or Claude Code bridge).
#[derive(Debug, Clone)]
pub struct JobEventRecord {
    pub id: i64,
    pub job_id: Uuid,
    pub event_type: String,
    pub data: serde_json::Value,
    pub created_at: DateTime<Utc>,
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
            key: r.get("key"),
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
                key: r.get("key"),
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
    ///
    /// Each entry is upserted individually within a single transaction.
    pub async fn set_all_settings(
        &self,
        user_id: UserId,
        settings: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await?;

        for (key, value) in settings {
            tx.execute(
                r#"
                INSERT INTO settings (user_id, key, value, updated_at)
                VALUES ($1, $2, $3, NOW())
                ON CONFLICT (user_id, key) DO UPDATE SET
                    value = EXCLUDED.value,
                    updated_at = NOW()
                "#,
                &[&user_id.as_str(), &key, value],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Check if the settings table has any rows for a user.
    pub async fn has_settings(&self, user_id: UserId) -> Result<bool, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_one(
                "SELECT COUNT(*) as cnt FROM settings WHERE user_id = $1",
                &[&user_id.as_str()],
            )
            .await?;
        let count: i64 = row.get("cnt");
        Ok(count > 0)
    }
}

// ==================== Routines ====================

#[cfg(feature = "postgres")]
use crate::agent::routine::{
    NotifyConfig, Routine, RoutineAction, RoutineGuardrails, RoutineRun, RunStatus, Trigger,
};

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
            key: r.get("key"),
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
                key: r.get("key"),
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
    ///
    /// Each entry is upserted individually within a single transaction.
    pub async fn set_all_settings(
        &self,
        user_id: UserId,
        settings: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await?;

        for (key, value) in settings {
            tx.execute(
                r#"
                INSERT INTO settings (user_id, key, value, updated_at)
                VALUES ($1, $2, $3, NOW())
                ON CONFLICT (user_id, key) DO UPDATE SET
                    value = EXCLUDED.value,
                    updated_at = NOW()
                "#,
                &[&user_id.as_str(), &key, value],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Check if the settings table has any rows for a user.
    pub async fn has_settings(&self, user_id: UserId) -> Result<bool, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_one(
                "SELECT COUNT(*) as cnt FROM settings WHERE user_id = $1",
                &[&user_id.as_str()],
            )
            .await?;
        let count: i64 = row.get("cnt");
        Ok(count > 0)
    }
}

#[cfg(feature = "postgres")]
fn row_to_routine(row: &tokio_postgres::Row) -> Result<Routine, DatabaseError> {
    let trigger_type: String = row.get("trigger_type");
    let trigger_config: serde_json::Value = row.get("trigger_config");
    let action_type: String = row.get("action_type");
    let action_config: serde_json::Value = row.get("action_config");
    let cooldown_secs: i32 = row.get("cooldown_secs");
    let max_concurrent: i32 = row.get("max_concurrent");
    let dedup_window_secs: Option<i32> = row.get("dedup_window_secs");

    let trigger = Trigger::from_db(&trigger_type, trigger_config)
        .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
    let action = RoutineAction::from_db(&action_type, action_config)
        .map_err(|e| DatabaseError::Serialization(e.to_string()))?;

    Ok(Routine {
        id: row.get("id"),
        name: row.get("name"),
        description: row.get("description"),
        user_id: row.get("user_id"),
        enabled: row.get("enabled"),
        trigger,
        action,
        guardrails: RoutineGuardrails {
            cooldown: std::time::Duration::from_secs(cooldown_secs as u64),
            max_concurrent: max_concurrent as u32,
            dedup_window: dedup_window_secs.map(|s| std::time::Duration::from_secs(s as u64)),
        },
        notify: NotifyConfig {
            channel: row.get("notify_channel"),
            user: row.get("notify_user"),
            on_attention: row.get("notify_on_attention"),
            on_failure: row.get("notify_on_failure"),
            on_success: row.get("notify_on_success"),
        },
        last_run_at: row.get("last_run_at"),
        next_fire_at: row.get("next_fire_at"),
        run_count: row.get::<_, i64>("run_count") as u64,
        consecutive_failures: row.get::<_, i32>("consecutive_failures") as u32,
        state: row.get("state"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

#[cfg(feature = "postgres")]
fn row_to_routine_run(row: &tokio_postgres::Row) -> Result<RoutineRun, DatabaseError> {
    let status_str: String = row.get("status");
    let status: RunStatus = status_str
        .parse()
        .map_err(|e: crate::error::RoutineError| DatabaseError::Serialization(e.to_string()))?;

    Ok(RoutineRun {
        id: row.get("id"),
        routine_id: row.get("routine_id"),
        trigger_type: row.get("trigger_type"),
        trigger_detail: row.get("trigger_detail"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
        status,
        result_summary: row.get("result_summary"),
        tokens_used: row.get("tokens_used"),
        job_id: row.get("job_id"),
        created_at: row.get("created_at"),
    })
}

// ==================== Conversation Persistence ====================

/// Summary of a conversation for the thread list.
#[derive(Debug, Clone)]
pub struct ConversationSummary {
    pub id: Uuid,
    /// First user message, truncated to 100 chars.
    pub title: Option<String>,
    pub message_count: i64,
    pub started_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    /// Thread type extracted from metadata (e.g. "assistant", "thread").
    pub thread_type: Option<String>,
    /// Channel that owns this conversation (e.g. "gateway", "telegram", "routine").
    pub channel: String,
}

/// A single message in a conversation.
#[derive(Debug, Clone)]
pub struct ConversationMessage {
    pub id: Uuid,
    pub role: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
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
            key: r.get("key"),
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
                key: r.get("key"),
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
    ///
    /// Each entry is upserted individually within a single transaction.
    pub async fn set_all_settings(
        &self,
        user_id: UserId,
        settings: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await?;

        for (key, value) in settings {
            tx.execute(
                r#"
                INSERT INTO settings (user_id, key, value, updated_at)
                VALUES ($1, $2, $3, NOW())
                ON CONFLICT (user_id, key) DO UPDATE SET
                    value = EXCLUDED.value,
                    updated_at = NOW()
                "#,
                &[&user_id.as_str(), &key, value],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Check if the settings table has any rows for a user.
    pub async fn has_settings(&self, user_id: UserId) -> Result<bool, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_one(
                "SELECT COUNT(*) as cnt FROM settings WHERE user_id = $1",
                &[&user_id.as_str()],
            )
            .await?;
        let count: i64 = row.get("cnt");
        Ok(count > 0)
    }
}

#[cfg(feature = "postgres")]
fn parse_job_state(s: &str) -> JobState {
    s.parse().unwrap_or(JobState::Pending)
}

// ==================== Tool Failures ====================

#[cfg(feature = "postgres")]
use crate::agent::BrokenTool;

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
            key: r.get("key"),
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
                key: r.get("key"),
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
    ///
    /// Each entry is upserted individually within a single transaction.
    pub async fn set_all_settings(
        &self,
        user_id: UserId,
        settings: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await?;

        for (key, value) in settings {
            tx.execute(
                r#"
                INSERT INTO settings (user_id, key, value, updated_at)
                VALUES ($1, $2, $3, NOW())
                ON CONFLICT (user_id, key) DO UPDATE SET
                    value = EXCLUDED.value,
                    updated_at = NOW()
                "#,
                &[&user_id.as_str(), &key, value],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Check if the settings table has any rows for a user.
    pub async fn has_settings(&self, user_id: UserId) -> Result<bool, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_one(
                "SELECT COUNT(*) as cnt FROM settings WHERE user_id = $1",
                &[&user_id.as_str()],
            )
            .await?;
        let count: i64 = row.get("cnt");
        Ok(count > 0)
    }
}

// ==================== Settings ====================

/// A single setting row from the database.
#[derive(Debug, Clone)]
pub struct SettingRow {
    pub key: String,
    pub value: serde_json::Value,
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
            key: r.get("key"),
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
                key: r.get("key"),
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
    ///
    /// Each entry is upserted individually within a single transaction.
    pub async fn set_all_settings(
        &self,
        user_id: UserId,
        settings: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Result<(), DatabaseError> {
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await?;

        for (key, value) in settings {
            tx.execute(
                r#"
                INSERT INTO settings (user_id, key, value, updated_at)
                VALUES ($1, $2, $3, NOW())
                ON CONFLICT (user_id, key) DO UPDATE SET
                    value = EXCLUDED.value,
                    updated_at = NOW()
                "#,
                &[&user_id.as_str(), &key, value],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Check if the settings table has any rows for a user.
    pub async fn has_settings(&self, user_id: UserId) -> Result<bool, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_one(
                "SELECT COUNT(*) as cnt FROM settings WHERE user_id = $1",
                &[&user_id.as_str()],
            )
            .await?;
        let count: i64 = row.get("cnt");
        Ok(count > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation_summary_has_channel_field() {
        // Regression: ConversationSummary must include a `channel` field
        // so the gateway can distinguish thread origins.
        let summary = ConversationSummary {
            id: Uuid::nil(),
            title: Some("Hello".to_string()),
            message_count: 1,
            started_at: Utc::now(),
            last_activity: Utc::now(),
            thread_type: Some("thread".to_string()),
            channel: "telegram".to_string(),
        };
        assert_eq!(summary.channel, "telegram");
    }

    #[test]
    fn test_conversation_summary_channel_various_values() {
        for ch in ["gateway", "routine", "heartbeat", "telegram", "signal"] {
            let summary = ConversationSummary {
                id: Uuid::nil(),
                title: None,
                message_count: 0,
                started_at: Utc::now(),
                last_activity: Utc::now(),
                thread_type: None,
                channel: ch.to_string(),
            };
            assert_eq!(summary.channel, ch);
        }
    }

    /// Regression test: save_job must persist user_id and get_job must return it.
    /// Requires a running PostgreSQL instance (integration tier).
    #[cfg(feature = "postgres")]
    #[tokio::test]
    #[ignore]
    async fn test_save_job_persists_user_id() {
        use crate::config::Config;
        use crate::context::JobContext;

        let _ = dotenvy::dotenv();
        let config = Config::from_env().await.expect("Failed to load config");
        let store = Store::new(&config.database)
            .await
            .expect("Failed to connect to database");
        store
            .run_migrations()
            .await
            .expect("Failed to run migrations");

        let ctx = JobContext::with_user("test-user-42", "PG user_id test", "regression test");
        store.save_job(&ctx).await.unwrap();

        let loaded = store.get_job(ctx.job_id).await.unwrap().unwrap();
        assert_eq!(loaded.user_id, "test-user-42");

        // Clean up
        let conn = store.conn().await.unwrap();
        conn.execute("DELETE FROM agent_jobs WHERE id = $1", &[&ctx.job_id])
            .await
            .unwrap();
    }
}
