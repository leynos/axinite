//! Sandbox-job and job-event persistence helpers.

use chrono::{DateTime, Utc};
use uuid::Uuid;

#[cfg(feature = "postgres")]
#[path = "sandbox/events.rs"]
mod events;

#[cfg(feature = "postgres")]
use super::Store;
#[cfg(feature = "postgres")]
use crate::db::{SandboxJobStatusUpdate, SandboxMode, UserId};
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;

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

impl SandboxJobSummary {
    pub fn add_count(&mut self, status: &str, count: usize) {
        self.total += count;
        match status {
            "creating" => self.creating += count,
            "running" => self.running += count,
            "completed" => self.completed += count,
            "failed" => self.failed += count,
            "interrupted" => self.interrupted += count,
            _ => {}
        }
    }
}

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
fn row_to_sandbox_job(r: &tokio_postgres::Row) -> SandboxJobRecord {
    SandboxJobRecord {
        id: r.get("id"),
        task: r.get("title"),
        status: r.get("status"),
        user_id: r.get("user_id"),
        project_dir: r
            .get::<_, Option<String>>("project_dir")
            .unwrap_or_default(),
        success: r.get("success"),
        failure_reason: r.get("failure_reason"),
        created_at: r.get("created_at"),
        started_at: r.get("started_at"),
        completed_at: r.get("completed_at"),
        credential_grants_json: r.get::<_, String>("description"),
    }
}

#[cfg(feature = "postgres")]
impl Store {
    /// Insert a new sandbox job into `agent_jobs`.
    pub async fn save_sandbox_job(&self, job: &SandboxJobRecord) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        conn.execute(
            r#"
            INSERT INTO agent_jobs (
                id, title, description, status, source, user_id, project_dir,
                success, failure_reason, created_at, started_at, completed_at
            ) VALUES ($1, $2, $3, $4, 'sandbox', $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (id) DO UPDATE SET
                title = EXCLUDED.title,
                description = EXCLUDED.description,
                user_id = EXCLUDED.user_id,
                project_dir = EXCLUDED.project_dir,
                status = EXCLUDED.status,
                success = EXCLUDED.success,
                failure_reason = EXCLUDED.failure_reason,
                started_at = EXCLUDED.started_at,
                completed_at = EXCLUDED.completed_at
            "#,
            &[
                &job.id,
                &job.task,
                &job.credential_grants_json,
                &job.status,
                &job.user_id,
                &job.project_dir,
                &job.success,
                &job.failure_reason,
                &job.created_at,
                &job.started_at,
                &job.completed_at,
            ],
        )
        .await?;
        Ok(())
    }

    /// Get a sandbox job by ID.
    pub async fn get_sandbox_job(
        &self,
        id: Uuid,
    ) -> Result<Option<SandboxJobRecord>, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_opt(
                r#"
                SELECT id, title, description, status, user_id, project_dir,
                       success, failure_reason, created_at, started_at, completed_at
                FROM agent_jobs WHERE id = $1 AND source = 'sandbox'
                "#,
                &[&id],
            )
            .await?;

        Ok(row.as_ref().map(row_to_sandbox_job))
    }

    /// List all sandbox jobs, most recent first.
    pub async fn list_sandbox_jobs(&self) -> Result<Vec<SandboxJobRecord>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                r#"
                SELECT id, title, description, status, user_id, project_dir,
                       success, failure_reason, created_at, started_at, completed_at
                FROM agent_jobs WHERE source = 'sandbox'
                ORDER BY created_at DESC
                "#,
                &[],
            )
            .await?;

        Ok(rows.iter().map(row_to_sandbox_job).collect())
    }

    /// List sandbox jobs for a specific user, most recent first.
    pub async fn list_sandbox_jobs_for_user(
        &self,
        user_id: UserId,
    ) -> Result<Vec<SandboxJobRecord>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                r#"
                SELECT id, title, description, status, user_id, project_dir,
                       success, failure_reason, created_at, started_at, completed_at
                FROM agent_jobs WHERE source = 'sandbox' AND user_id = $1
                ORDER BY created_at DESC
                "#,
                &[&user_id.as_str()],
            )
            .await?;

        Ok(rows.iter().map(row_to_sandbox_job).collect())
    }

    /// Get a summary of sandbox job counts by status for a specific user.
    pub async fn sandbox_job_summary_for_user(
        &self,
        user_id: UserId,
    ) -> Result<SandboxJobSummary, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT status, COUNT(*) as cnt FROM agent_jobs WHERE source = 'sandbox' AND user_id = $1 GROUP BY status",
                &[&user_id.as_str()],
            )
            .await?;

        let mut summary = SandboxJobSummary::default();
        for row in &rows {
            summary.add_count(
                row.get::<_, &str>("status"),
                row.get::<_, i64>("cnt") as usize,
            );
        }
        Ok(summary)
    }

    /// Check if a sandbox job belongs to a specific user.
    pub async fn sandbox_job_belongs_to_user(
        &self,
        job_id: Uuid,
        user_id: UserId,
    ) -> Result<bool, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_opt(
                "SELECT 1 FROM agent_jobs WHERE id = $1 AND user_id = $2 AND source = 'sandbox'",
                &[&job_id, &user_id.as_str()],
            )
            .await?;
        Ok(row.is_some())
    }

    /// Update sandbox job status and optional timestamps/result.
    pub async fn update_sandbox_job_status(
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
        let conn = self.conn().await?;
        conn.execute(
            r#"
            UPDATE agent_jobs SET
                status = $2,
                success = COALESCE($3, success),
                failure_reason = COALESCE($4, failure_reason),
                started_at = COALESCE($5, started_at),
                completed_at = COALESCE($6, completed_at)
            WHERE id = $1 AND source = 'sandbox'
            "#,
            &[&id, &status, &success, &message, &started_at, &completed_at],
        )
        .await?;
        Ok(())
    }

    /// Mark any sandbox jobs left in "running" or "creating" as "interrupted".
    pub async fn cleanup_stale_sandbox_jobs(&self) -> Result<u64, DatabaseError> {
        let conn = self.conn().await?;
        let count = conn
            .execute(
                r#"
                UPDATE agent_jobs SET
                    status = 'interrupted',
                    failure_reason = 'Process restarted',
                    completed_at = NOW()
                WHERE source = 'sandbox' AND status IN ('running', 'creating')
                "#,
                &[],
            )
            .await?;
        if count > 0 {
            tracing::info!("Marked {} stale sandbox jobs as interrupted", count);
        }
        Ok(count)
    }

    /// Get a summary of sandbox job counts by status.
    pub async fn sandbox_job_summary(&self) -> Result<SandboxJobSummary, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT status, COUNT(*) as cnt FROM agent_jobs WHERE source = 'sandbox' GROUP BY status",
                &[],
            )
            .await?;

        let mut summary = SandboxJobSummary::default();
        for row in &rows {
            summary.add_count(
                row.get::<_, &str>("status"),
                row.get::<_, i64>("cnt") as usize,
            );
        }
        Ok(summary)
    }

    /// Update the job_mode column for a sandbox job.
    pub async fn update_sandbox_job_mode(
        &self,
        id: Uuid,
        mode: SandboxMode,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        conn.execute(
            "UPDATE agent_jobs SET job_mode = $2 WHERE id = $1 AND source = 'sandbox'",
            &[&id, &mode.as_str()],
        )
        .await?;
        Ok(())
    }

    /// Get the job_mode for a sandbox job.
    pub async fn get_sandbox_job_mode(
        &self,
        id: Uuid,
    ) -> Result<Option<SandboxMode>, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_opt(
                "SELECT job_mode FROM agent_jobs WHERE id = $1 AND source = 'sandbox'",
                &[&id],
            )
            .await?;
        row.and_then(|r| r.get::<_, Option<String>>("job_mode"))
            .map(|mode| SandboxMode::try_from(mode.as_str()).map_err(DatabaseError::Serialization))
            .transpose()
    }
}
