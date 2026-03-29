//! Routine persistence helpers.

#[cfg(feature = "postgres")]
use chrono::{DateTime, Utc};
#[cfg(feature = "postgres")]
use uuid::Uuid;

#[cfg(feature = "postgres")]
use super::Store;
#[cfg(feature = "postgres")]
use crate::agent::routine::{
    NotifyConfig, Routine, RoutineAction, RoutineGuardrails, RoutineRun, RunStatus, Trigger,
};
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;

#[cfg(feature = "postgres")]
impl Store {
    /// Create a new routine.
    pub async fn create_routine(&self, routine: &Routine) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        let trigger_type = routine.trigger.type_tag();
        let trigger_config = routine.trigger.to_config_json();
        let action_type = routine.action.type_tag();
        let action_config = routine.action.to_config_json();
        let cooldown_secs = routine.guardrails.cooldown.as_secs() as i32;
        let max_concurrent = routine.guardrails.max_concurrent as i32;
        let dedup_window_secs = routine.guardrails.dedup_window.map(|d| d.as_secs() as i32);

        conn.execute(
            r#"
            INSERT INTO routines (
                id, name, description, user_id, enabled,
                trigger_type, trigger_config, action_type, action_config,
                cooldown_secs, max_concurrent, dedup_window_secs,
                notify_channel, notify_user, notify_on_success, notify_on_failure, notify_on_attention,
                state, next_fire_at, created_at, updated_at
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9,
                $10, $11, $12,
                $13, $14, $15, $16, $17,
                $18, $19, $20, $21
            )
            "#,
            &[
                &routine.id,
                &routine.name,
                &routine.description,
                &routine.user_id,
                &routine.enabled,
                &trigger_type,
                &trigger_config,
                &action_type,
                &action_config,
                &cooldown_secs,
                &max_concurrent,
                &dedup_window_secs,
                &routine.notify.channel,
                &routine.notify.user,
                &routine.notify.on_success,
                &routine.notify.on_failure,
                &routine.notify.on_attention,
                &routine.state,
                &routine.next_fire_at,
                &routine.created_at,
                &routine.updated_at,
            ],
        )
        .await?;

        Ok(())
    }

    /// Get a routine by ID.
    pub async fn get_routine(&self, id: Uuid) -> Result<Option<Routine>, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_opt("SELECT * FROM routines WHERE id = $1", &[&id])
            .await?;
        row.map(|r| row_to_routine(&r)).transpose()
    }

    /// Get a routine by user_id and name.
    pub async fn get_routine_by_name(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<Option<Routine>, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_opt(
                "SELECT * FROM routines WHERE user_id = $1 AND name = $2",
                &[&user_id, &name],
            )
            .await?;
        row.map(|r| row_to_routine(&r)).transpose()
    }

    /// List routines for a user.
    pub async fn list_routines(&self, user_id: &str) -> Result<Vec<Routine>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT * FROM routines WHERE user_id = $1 ORDER BY name",
                &[&user_id],
            )
            .await?;
        rows.iter().map(row_to_routine).collect()
    }

    /// List all routines across all users.
    pub async fn list_all_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query("SELECT * FROM routines ORDER BY name", &[])
            .await?;
        rows.iter().map(row_to_routine).collect()
    }

    /// List all enabled routines with event triggers (for event matching).
    pub async fn list_event_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT * FROM routines WHERE enabled AND trigger_type IN ('event', 'system_event')",
                &[],
            )
            .await?;
        rows.iter().map(row_to_routine).collect()
    }

    /// List all enabled cron routines whose next_fire_at <= now.
    pub async fn list_due_cron_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        let conn = self.conn().await?;
        let now = Utc::now();
        let rows = conn
            .query(
                r#"
                SELECT * FROM routines
                WHERE enabled
                  AND trigger_type = 'cron'
                  AND next_fire_at IS NOT NULL
                  AND next_fire_at <= $1
                "#,
                &[&now],
            )
            .await?;
        rows.iter().map(row_to_routine).collect()
    }

    /// Update a routine (full replacement of mutable fields).
    pub async fn update_routine(&self, routine: &Routine) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        let trigger_type = routine.trigger.type_tag();
        let trigger_config = routine.trigger.to_config_json();
        let action_type = routine.action.type_tag();
        let action_config = routine.action.to_config_json();
        let cooldown_secs = routine.guardrails.cooldown.as_secs() as i32;
        let max_concurrent = routine.guardrails.max_concurrent as i32;
        let dedup_window_secs = routine.guardrails.dedup_window.map(|d| d.as_secs() as i32);

        conn.execute(
            r#"
            UPDATE routines SET
                name = $2, description = $3, enabled = $4,
                trigger_type = $5, trigger_config = $6,
                action_type = $7, action_config = $8,
                cooldown_secs = $9, max_concurrent = $10, dedup_window_secs = $11,
                notify_channel = $12, notify_user = $13,
                notify_on_success = $14, notify_on_failure = $15, notify_on_attention = $16,
                state = $17, next_fire_at = $18,
                updated_at = now()
            WHERE id = $1
            "#,
            &[
                &routine.id,
                &routine.name,
                &routine.description,
                &routine.enabled,
                &trigger_type,
                &trigger_config,
                &action_type,
                &action_config,
                &cooldown_secs,
                &max_concurrent,
                &dedup_window_secs,
                &routine.notify.channel,
                &routine.notify.user,
                &routine.notify.on_success,
                &routine.notify.on_failure,
                &routine.notify.on_attention,
                &routine.state,
                &routine.next_fire_at,
            ],
        )
        .await?;
        Ok(())
    }

    /// Update runtime state after a routine fires.
    pub async fn update_routine_runtime(
        &self,
        id: Uuid,
        last_run_at: DateTime<Utc>,
        next_fire_at: Option<DateTime<Utc>>,
        run_count: u64,
        consecutive_failures: u32,
        state: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        conn.execute(
            r#"
            UPDATE routines SET
                last_run_at = $2, next_fire_at = $3,
                run_count = $4, consecutive_failures = $5,
                state = $6, updated_at = now()
            WHERE id = $1
            "#,
            &[
                &id,
                &last_run_at,
                &next_fire_at,
                &(run_count as i64),
                &(consecutive_failures as i32),
                state,
            ],
        )
        .await?;
        Ok(())
    }

    /// Delete a routine.
    pub async fn delete_routine(&self, id: Uuid) -> Result<bool, DatabaseError> {
        let conn = self.conn().await?;
        let count = conn
            .execute("DELETE FROM routines WHERE id = $1", &[&id])
            .await?;
        Ok(count > 0)
    }

    /// Record a routine run starting.
    pub async fn create_routine_run(&self, run: &RoutineRun) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        let status = run.status.to_string();
        conn.execute(
            r#"
            INSERT INTO routine_runs (
                id, routine_id, trigger_type, trigger_detail,
                started_at, status, job_id
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            &[
                &run.id,
                &run.routine_id,
                &run.trigger_type,
                &run.trigger_detail,
                &run.started_at,
                &status,
                &run.job_id,
            ],
        )
        .await?;
        Ok(())
    }

    /// Complete a routine run.
    pub async fn complete_routine_run(
        &self,
        id: Uuid,
        status: RunStatus,
        result_summary: Option<&str>,
        tokens_used: Option<i32>,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        let status_str = status.to_string();
        let now = Utc::now();
        conn.execute(
            r#"
            UPDATE routine_runs SET
                completed_at = $2, status = $3,
                result_summary = $4, tokens_used = $5
            WHERE id = $1
            "#,
            &[&id, &now, &status_str, &result_summary, &tokens_used],
        )
        .await?;
        Ok(())
    }

    /// List recent runs for a routine.
    pub async fn list_routine_runs(
        &self,
        routine_id: Uuid,
        limit: i64,
    ) -> Result<Vec<RoutineRun>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                r#"
                SELECT * FROM routine_runs
                WHERE routine_id = $1
                ORDER BY started_at DESC
                LIMIT $2
                "#,
                &[&routine_id, &limit],
            )
            .await?;
        rows.iter().map(row_to_routine_run).collect()
    }

    /// Count currently running runs for a routine.
    pub async fn count_running_routine_runs(&self, routine_id: Uuid) -> Result<i64, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_one(
                "SELECT COUNT(*) as cnt FROM routine_runs WHERE routine_id = $1 AND status = 'running'",
                &[&routine_id],
            )
            .await?;
        Ok(row.get("cnt"))
    }

    /// Link a routine run to a dispatched job.
    pub async fn link_routine_run_to_job(
        &self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        conn.execute(
            "UPDATE routine_runs SET job_id = $1 WHERE id = $2",
            &[&job_id, &run_id],
        )
        .await?;
        Ok(())
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
