//! Sandbox-related SandboxStore implementation for LibSqlBackend.

use chrono::Utc;
use libsql::params;
use uuid::Uuid;

mod events;

use super::{
    LibSqlBackend, fmt_opt_ts, fmt_ts, get_i64, get_opt_bool, get_opt_text, get_opt_ts, get_text,
    get_ts, opt_text,
};
use crate::db::{
    NativeSandboxStore, SandboxEventType, SandboxJobStatusUpdate, SandboxMode, UserId,
};
use crate::error::DatabaseError;
use crate::history::{JobEventRecord, SandboxJobRecord, SandboxJobSummary};

impl NativeSandboxStore for LibSqlBackend {
    async fn save_sandbox_job(&self, job: &SandboxJobRecord) -> Result<(), DatabaseError> {
        let conn = self.connect().await?;
        let rows_affected = conn
            .execute(
                r#"
                INSERT INTO agent_jobs (
                    id, title, description, status, source, user_id, project_dir,
                    success, failure_reason, created_at, started_at, completed_at
                ) VALUES (?1, ?2, ?3, ?4, 'sandbox', ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                ON CONFLICT (id) DO UPDATE SET
                    title = excluded.title,
                    description = excluded.description,
                    user_id = excluded.user_id,
                    project_dir = excluded.project_dir,
                    status = excluded.status,
                    success = excluded.success,
                    failure_reason = excluded.failure_reason,
                    started_at = excluded.started_at,
                    completed_at = excluded.completed_at
                WHERE agent_jobs.source = 'sandbox'
                "#,
                params![
                    job.id.to_string(),
                    job.task.as_str(),
                    job.credential_grants_json.as_str(),
                    job.status.as_str(),
                    job.user_id.as_str(),
                    job.project_dir.as_str(),
                    job.success.map(|b| b as i64),
                    opt_text(job.failure_reason.as_deref()),
                    fmt_ts(&job.created_at),
                    fmt_opt_ts(&job.started_at),
                    fmt_opt_ts(&job.completed_at),
                ],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
        if rows_affected == 0 {
            return Err(DatabaseError::Query(format!(
                "sandbox job {} conflicts with a non-sandbox agent_jobs row",
                job.id
            )));
        }
        Ok(())
    }

    async fn get_sandbox_job(&self, id: Uuid) -> Result<Option<SandboxJobRecord>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, title, description, status, user_id, project_dir,
                       success, failure_reason, created_at, started_at, completed_at
                FROM agent_jobs WHERE id = ?1 AND source = 'sandbox'
                "#,
                params![id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            Some(row) => Ok(Some(SandboxJobRecord {
                id: get_text(&row, 0).parse().map_err(|error| {
                    DatabaseError::Serialization(format!(
                        "invalid agent_jobs.id UUID '{}': {error}",
                        get_text(&row, 0)
                    ))
                })?,
                task: get_text(&row, 1),
                credential_grants_json: get_text(&row, 2),
                status: get_text(&row, 3),
                user_id: UserId::from(get_text(&row, 4)),
                project_dir: get_text(&row, 5),
                success: get_opt_bool(&row, 6),
                failure_reason: get_opt_text(&row, 7),
                created_at: get_ts(&row, 8),
                started_at: get_opt_ts(&row, 9),
                completed_at: get_opt_ts(&row, 10),
            })),
            None => Ok(None),
        }
    }

    async fn list_sandbox_jobs(&self) -> Result<Vec<SandboxJobRecord>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, title, description, status, user_id, project_dir,
                       success, failure_reason, created_at, started_at, completed_at
                FROM agent_jobs WHERE source = 'sandbox'
                ORDER BY created_at DESC, id DESC
                "#,
                (),
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut jobs = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            let id_raw = get_text(&row, 0);
            jobs.push(SandboxJobRecord {
                id: id_raw.parse().map_err(|error| {
                    DatabaseError::Serialization(format!(
                        "invalid agent_jobs.id UUID '{id_raw}': {error}"
                    ))
                })?,
                task: get_text(&row, 1),
                credential_grants_json: get_text(&row, 2),
                status: get_text(&row, 3),
                user_id: UserId::from(get_text(&row, 4)),
                project_dir: get_text(&row, 5),
                success: get_opt_bool(&row, 6),
                failure_reason: get_opt_text(&row, 7),
                created_at: get_ts(&row, 8),
                started_at: get_opt_ts(&row, 9),
                completed_at: get_opt_ts(&row, 10),
            });
        }
        Ok(jobs)
    }

    async fn update_sandbox_job_status(
        &self,
        params: SandboxJobStatusUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        let SandboxJobStatusUpdate {
            id,
            status,
            success,
            message,
            started_at,
            completed_at,
        } = params;
        let conn = self.connect().await?;
        conn.execute(
            r#"
                UPDATE agent_jobs SET
                    status = ?2,
                    success = COALESCE(?3, success),
                    failure_reason = COALESCE(?4, failure_reason),
                    started_at = COALESCE(?5, started_at),
                    completed_at = COALESCE(?6, completed_at)
                WHERE id = ?1 AND source = 'sandbox'
                "#,
            params![
                id.to_string(),
                status.as_str(),
                success.map(|b| b as i64),
                message,
                fmt_opt_ts(&started_at),
                fmt_opt_ts(&completed_at),
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(())
    }

    async fn cleanup_stale_sandbox_jobs(&self) -> Result<u64, DatabaseError> {
        let conn = self.connect().await?;
        let now = fmt_ts(&Utc::now());
        let count = conn
            .execute(
                r#"
                UPDATE agent_jobs SET
                    status = 'interrupted',
                    failure_reason = 'Process restarted',
                    completed_at = ?1
                WHERE source = 'sandbox' AND status IN ('running', 'creating')
                "#,
                params![now],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
        if count > 0 {
            tracing::info!("Marked {} stale sandbox jobs as interrupted", count);
        }
        Ok(count)
    }

    async fn sandbox_job_summary(&self) -> Result<SandboxJobSummary, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT status, COUNT(*) as cnt FROM agent_jobs WHERE source = 'sandbox' GROUP BY status",
                (),
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut summary = SandboxJobSummary::default();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            let status = get_text(&row, 0);
            let count = usize::try_from(get_i64(&row, 1)).map_err(|error| {
                DatabaseError::Serialization(format!(
                    "sandbox job summary count exceeds usize range: {error}"
                ))
            })?;
            summary.add_count(&status, count);
        }
        Ok(summary)
    }

    async fn list_sandbox_jobs_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<SandboxJobRecord>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, title, description, status, user_id, project_dir,
                       success, failure_reason, created_at, started_at, completed_at
                FROM agent_jobs WHERE source = 'sandbox' AND user_id = ?1
                ORDER BY created_at DESC, id DESC
                "#,
                libsql::params![user_id.as_str()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut jobs = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            let id_raw = get_text(&row, 0);
            jobs.push(SandboxJobRecord {
                id: id_raw.parse().map_err(|error| {
                    DatabaseError::Serialization(format!(
                        "invalid agent_jobs.id UUID '{id_raw}': {error}"
                    ))
                })?,
                task: get_text(&row, 1),
                credential_grants_json: get_text(&row, 2),
                status: get_text(&row, 3),
                user_id: UserId::from(get_text(&row, 4)),
                project_dir: get_text(&row, 5),
                success: get_opt_bool(&row, 6),
                failure_reason: get_opt_text(&row, 7),
                created_at: get_ts(&row, 8),
                started_at: get_opt_ts(&row, 9),
                completed_at: get_opt_ts(&row, 10),
            });
        }
        Ok(jobs)
    }

    async fn sandbox_job_summary_for_user(
        &self,
        user_id: UserId,
    ) -> Result<SandboxJobSummary, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT status, COUNT(*) as cnt FROM agent_jobs WHERE source = 'sandbox' AND user_id = ?1 GROUP BY status",
                libsql::params![user_id.as_str()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut summary = SandboxJobSummary::default();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            let status = get_text(&row, 0);
            let count = usize::try_from(get_i64(&row, 1)).map_err(|error| {
                DatabaseError::Serialization(format!(
                    "sandbox job summary count exceeds usize range: {error}"
                ))
            })?;
            summary.add_count(&status, count);
        }
        Ok(summary)
    }

    async fn sandbox_job_belongs_to_user(
        &self,
        job_id: Uuid,
        user_id: UserId,
    ) -> Result<bool, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT 1 FROM agent_jobs WHERE id = ?1 AND user_id = ?2 AND source = 'sandbox'",
                libsql::params![job_id.to_string(), user_id.as_str()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
        let found = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(found.is_some())
    }

    async fn update_sandbox_job_mode(
        &self,
        id: Uuid,
        mode: SandboxMode,
    ) -> Result<(), DatabaseError> {
        let conn = self.connect().await?;
        conn.execute(
            "UPDATE agent_jobs SET job_mode = ?2 WHERE id = ?1 AND source = 'sandbox'",
            params![id.to_string(), mode.as_str()],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(())
    }

    async fn get_sandbox_job_mode(&self, id: Uuid) -> Result<Option<SandboxMode>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT job_mode FROM agent_jobs WHERE id = ?1 AND source = 'sandbox'",
                params![id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            Some(row) => row
                .get::<Option<String>>(0)
                .map_err(|e| DatabaseError::Query(e.to_string()))?
                .map(|mode| {
                    SandboxMode::try_from(mode.as_str())
                        .map_err(|error| DatabaseError::Serialization(error.to_string()))
                })
                .transpose(),
            None => Ok(None),
        }
    }

    async fn save_job_event(
        &self,
        job_id: Uuid,
        event_type: SandboxEventType,
        data: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        events::save_job_event(self, job_id, event_type, data).await
    }

    async fn list_job_events(
        &self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<JobEventRecord>, DatabaseError> {
        events::list_job_events(self, job_id, before_id, limit).await
    }
}
