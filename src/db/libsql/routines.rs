//! Routine-related RoutineStore implementation for LibSqlBackend.

#[path = "routines/mapping.rs"]
mod mapping;
#[path = "routines/routine_runs.rs"]
mod routine_runs;

use chrono::Utc;
use libsql::params;
use uuid::Uuid;

use super::{LibSqlBackend, fmt_opt_ts, fmt_ts, get_i64, get_text, opt_text, opt_text_owned};
use crate::agent::routine::{Routine, RoutineRun};
use crate::db::{NativeRoutineStore, RoutineRunCompletion, RoutineRuntimeUpdate};
use crate::error::DatabaseError;

impl NativeRoutineStore for LibSqlBackend {
    async fn create_routine(&self, routine: &Routine) -> Result<(), DatabaseError> {
        let conn = self.connect().await?;
        let trigger_type = routine.trigger.type_tag();
        let trigger_config = routine.trigger.to_config_json();
        let action_type = routine.action.type_tag();
        let action_config = routine.action.to_config_json();
        let cooldown_secs = routine.guardrails.cooldown.as_secs() as i64;
        let max_concurrent = routine.guardrails.max_concurrent as i64;
        let dedup_window_secs = routine.guardrails.dedup_window.map(|d| d.as_secs() as i64);

        conn.execute(
                r#"
                INSERT INTO routines (
                    id, name, description, user_id, enabled,
                    trigger_type, trigger_config, action_type, action_config,
                    cooldown_secs, max_concurrent, dedup_window_secs,
                    notify_channel, notify_user, notify_on_success, notify_on_failure, notify_on_attention,
                    state, next_fire_at, created_at, updated_at
                ) VALUES (
                    ?1, ?2, ?3, ?4, ?5,
                    ?6, ?7, ?8, ?9,
                    ?10, ?11, ?12,
                    ?13, ?14, ?15, ?16, ?17,
                    ?18, ?19, ?20, ?21
                )
                "#,
                params![
                    routine.id.to_string(),
                    routine.name.as_str(),
                    routine.description.as_str(),
                    routine.user_id.as_str(),
                    routine.enabled as i64,
                    trigger_type,
                    trigger_config.to_string(),
                    action_type,
                    action_config.to_string(),
                    cooldown_secs,
                    max_concurrent,
                    dedup_window_secs,
                    opt_text(routine.notify.channel.as_deref()),
                    routine.notify.user.as_str(),
                    routine.notify.on_success as i64,
                    routine.notify.on_failure as i64,
                    routine.notify.on_attention as i64,
                    routine.state.to_string(),
                    fmt_opt_ts(&routine.next_fire_at),
                    fmt_ts(&routine.created_at),
                    fmt_ts(&routine.updated_at),
                ],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(())
    }

    async fn get_routine(&self, id: Uuid) -> Result<Option<Routine>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {} FROM routines WHERE id = ?1",
                    mapping::ROUTINE_COLUMNS
                ),
                params![id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            Some(row) => Ok(Some(mapping::row_to_routine_libsql(&row)?)),
            None => Ok(None),
        }
    }

    async fn get_routine_by_name(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<Option<Routine>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {} FROM routines WHERE user_id = ?1 AND name = ?2",
                    mapping::ROUTINE_COLUMNS
                ),
                params![user_id, name],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            Some(row) => Ok(Some(mapping::row_to_routine_libsql(&row)?)),
            None => Ok(None),
        }
    }

    async fn list_routines(&self, user_id: &str) -> Result<Vec<Routine>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {} FROM routines WHERE user_id = ?1 ORDER BY name",
                    mapping::ROUTINE_COLUMNS
                ),
                params![user_id],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut routines = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            routines.push(mapping::row_to_routine_libsql(&row)?);
        }
        Ok(routines)
    }

    async fn list_all_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {} FROM routines ORDER BY name",
                    mapping::ROUTINE_COLUMNS
                ),
                (),
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut routines = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            routines.push(mapping::row_to_routine_libsql(&row)?);
        }
        Ok(routines)
    }

    async fn list_event_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {} FROM routines WHERE enabled = 1 AND trigger_type IN ('event', 'system_event')",
                    mapping::ROUTINE_COLUMNS
                ),
                (),
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut routines = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            routines.push(mapping::row_to_routine_libsql(&row)?);
        }
        Ok(routines)
    }

    async fn list_due_cron_routines(&self) -> Result<Vec<Routine>, DatabaseError> {
        let conn = self.connect().await?;
        let now = fmt_ts(&Utc::now());
        let lease_until = fmt_ts(&(Utc::now() + chrono::Duration::minutes(1)));
        conn.execute("BEGIN IMMEDIATE", params![])
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
        let result: Result<Vec<Routine>, DatabaseError> = async {
            let mut id_rows = conn
                .query(
                    "SELECT id FROM routines WHERE enabled = 1 AND trigger_type = 'cron' AND next_fire_at IS NOT NULL AND next_fire_at <= ?1",
                    params![now],
                )
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?;
            let mut due_ids = Vec::new();
            while let Some(row) = id_rows
                .next()
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?
            {
                due_ids.push(get_text(&row, 0));
            }

            let mut routines = Vec::new();
            for due_id in due_ids {
                let mut rows = conn
                    .query(
                        &format!(
                            "SELECT {} FROM routines WHERE id = ?1",
                            mapping::ROUTINE_COLUMNS
                        ),
                        params![due_id.clone()],
                    )
                    .await
                    .map_err(|e| DatabaseError::Query(e.to_string()))?;
                let row = rows
                    .next()
                    .await
                    .map_err(|e| DatabaseError::Query(e.to_string()))?
                    .ok_or_else(|| {
                        DatabaseError::Query(format!(
                            "due routine disappeared before it could be claimed: {due_id}"
                        ))
                    })?;
                let routine = mapping::row_to_routine_libsql(&row)?;
                conn.execute(
                    "UPDATE routines SET next_fire_at = ?2, updated_at = ?3 WHERE id = ?1",
                    params![routine.id.to_string(), lease_until.clone(), fmt_ts(&Utc::now())],
                )
                .await
                .map_err(|e| DatabaseError::Query(e.to_string()))?;
                routines.push(routine);
            }
            Ok(routines)
        }
        .await;

        match &result {
            Ok(_) => {
                conn.execute("COMMIT", params![])
                    .await
                    .map_err(|e| DatabaseError::Query(e.to_string()))?;
            }
            Err(_) => {
                let _ = conn.execute("ROLLBACK", params![]).await;
            }
        }

        result
    }

    async fn update_routine(&self, routine: &Routine) -> Result<(), DatabaseError> {
        let conn = self.connect().await?;
        let trigger_type = routine.trigger.type_tag();
        let trigger_config = routine.trigger.to_config_json();
        let action_type = routine.action.type_tag();
        let action_config = routine.action.to_config_json();
        let cooldown_secs = routine.guardrails.cooldown.as_secs() as i64;
        let max_concurrent = routine.guardrails.max_concurrent as i64;
        let dedup_window_secs = routine.guardrails.dedup_window.map(|d| d.as_secs() as i64);
        let now = fmt_ts(&Utc::now());

        conn.execute(
            r#"
                UPDATE routines SET
                    name = ?2, description = ?3, enabled = ?4,
                    trigger_type = ?5, trigger_config = ?6,
                    action_type = ?7, action_config = ?8,
                    cooldown_secs = ?9, max_concurrent = ?10, dedup_window_secs = ?11,
                    notify_channel = ?12, notify_user = ?13,
                    notify_on_success = ?14, notify_on_failure = ?15, notify_on_attention = ?16,
                    state = ?17, next_fire_at = ?18,
                    updated_at = ?19
                WHERE id = ?1
                "#,
            params![
                routine.id.to_string(),
                routine.name.as_str(),
                routine.description.as_str(),
                routine.enabled as i64,
                trigger_type,
                trigger_config.to_string(),
                action_type,
                action_config.to_string(),
                cooldown_secs,
                max_concurrent,
                dedup_window_secs,
                opt_text(routine.notify.channel.as_deref()),
                routine.notify.user.as_str(),
                routine.notify.on_success as i64,
                routine.notify.on_failure as i64,
                routine.notify.on_attention as i64,
                routine.state.to_string(),
                fmt_opt_ts(&routine.next_fire_at),
                now,
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(())
    }

    async fn update_routine_runtime(
        &self,
        params: RoutineRuntimeUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        let RoutineRuntimeUpdate {
            id,
            last_run_at,
            next_fire_at,
            run_count,
            consecutive_failures,
            state,
        } = params;
        let conn = self.connect().await?;
        let now = fmt_ts(&Utc::now());
        conn.execute(
            r#"
                UPDATE routines SET
                    last_run_at = ?2, next_fire_at = ?3,
                    run_count = ?4, consecutive_failures = ?5,
                    state = ?6, updated_at = ?7
                WHERE id = ?1
                "#,
            params![
                id.to_string(),
                fmt_ts(&last_run_at),
                fmt_opt_ts(&next_fire_at),
                run_count as i64,
                consecutive_failures as i64,
                state.to_string(),
                now,
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(())
    }

    async fn delete_routine(&self, id: Uuid) -> Result<bool, DatabaseError> {
        let conn = self.connect().await?;
        let count = conn
            .execute(
                "DELETE FROM routines WHERE id = ?1",
                params![id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(count > 0)
    }

    async fn create_routine_run(&self, run: &RoutineRun) -> Result<(), DatabaseError> {
        routine_runs::create_routine_run(self, run).await
    }

    async fn complete_routine_run(
        &self,
        params: RoutineRunCompletion<'_>,
    ) -> Result<(), DatabaseError> {
        routine_runs::complete_routine_run(self, params).await
    }

    async fn list_routine_runs(
        &self,
        routine_id: Uuid,
        limit: i64,
    ) -> Result<Vec<RoutineRun>, DatabaseError> {
        routine_runs::list_routine_runs(self, routine_id, limit).await
    }

    async fn count_running_routine_runs(&self, routine_id: Uuid) -> Result<i64, DatabaseError> {
        routine_runs::count_running_routine_runs(self, routine_id).await
    }

    async fn link_routine_run_to_job(
        &self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> Result<(), DatabaseError> {
        routine_runs::link_routine_run_to_job(self, run_id, job_id).await
    }
}
