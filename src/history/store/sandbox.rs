//! Sandbox-job and job-event persistence helpers.

use chrono::{DateTime, Utc};
use uuid::Uuid;

#[cfg(feature = "postgres")]
use super::Store;
#[cfg(feature = "postgres")]
use crate::db::{SandboxEventType, SandboxMode, UserId};
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

        Ok(row.map(|r| SandboxJobRecord {
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
        }))
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

        Ok(rows
            .iter()
            .map(|r| SandboxJobRecord {
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
            })
            .collect())
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

        Ok(rows
            .iter()
            .map(|r| SandboxJobRecord {
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
            })
            .collect())
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
            let status: String = row.get("status");
            let count: i64 = row.get("cnt");
            let c = count as usize;
            summary.total += c;
            match status.as_str() {
                "creating" => summary.creating += c,
                "running" => summary.running += c,
                "completed" => summary.completed += c,
                "failed" => summary.failed += c,
                "interrupted" => summary.interrupted += c,
                _ => {}
            }
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
        id: Uuid,
        status: &str,
        success: Option<bool>,
        message: Option<&str>,
        started_at: Option<DateTime<Utc>>,
        completed_at: Option<DateTime<Utc>>,
    ) -> Result<(), DatabaseError> {
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
            let status: String = row.get("status");
            let count: i64 = row.get("cnt");
            let c = count as usize;
            summary.total += c;
            match status.as_str() {
                "creating" => summary.creating += c,
                "running" => summary.running += c,
                "completed" => summary.completed += c,
                "failed" => summary.failed += c,
                "interrupted" => summary.interrupted += c,
                _ => {}
            }
        }
        Ok(summary)
    }

    /// Persist a job event (fire-and-forget from orchestrator handler).
    pub async fn save_job_event(
        &self,
        job_id: Uuid,
        event_type: SandboxEventType,
        data: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        conn.execute(
            r#"
            INSERT INTO job_events (job_id, event_type, data)
            VALUES ($1, $2, $3)
            "#,
            &[&job_id, &event_type.as_str(), data],
        )
        .await?;
        Ok(())
    }

    /// Load job events for a job, ordered by id.
    pub async fn list_job_events(
        &self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<JobEventRecord>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = match (before_id, limit) {
            (Some(before_id), Some(n)) => {
                conn.query(
                    r#"
                    SELECT id, job_id, event_type, data, created_at
                    FROM (
                        SELECT id, job_id, event_type, data, created_at
                        FROM job_events
                        WHERE job_id = $1 AND id < $2
                        ORDER BY id DESC
                        LIMIT $3
                    ) sub
                    ORDER BY id ASC
                    "#,
                    &[&job_id, &before_id, &n],
                )
                .await?
            }
            (Some(before_id), None) => {
                conn.query(
                    r#"
                    SELECT id, job_id, event_type, data, created_at
                    FROM job_events
                    WHERE job_id = $1 AND id < $2
                    ORDER BY id ASC
                    "#,
                    &[&job_id, &before_id],
                )
                .await?
            }
            (None, Some(n)) => {
                conn.query(
                    r#"
                    SELECT id, job_id, event_type, data, created_at
                    FROM (
                        SELECT id, job_id, event_type, data, created_at
                        FROM job_events
                        WHERE job_id = $1
                        ORDER BY id DESC
                        LIMIT $2
                    ) sub
                    ORDER BY id ASC
                    "#,
                    &[&job_id, &n],
                )
                .await?
            }
            (None, None) => {
                conn.query(
                    r#"
                    SELECT id, job_id, event_type, data, created_at
                    FROM job_events
                    WHERE job_id = $1
                    ORDER BY id ASC
                    "#,
                    &[&job_id],
                )
                .await?
            }
        };
        Ok(rows
            .iter()
            .map(|r| JobEventRecord {
                id: r.get("id"),
                job_id: r.get("job_id"),
                event_type: r.get("event_type"),
                data: r.get("data"),
                created_at: r.get("created_at"),
            })
            .collect())
    }

    /// Update the job_mode column for a sandbox job.
    pub async fn update_sandbox_job_mode(
        &self,
        id: Uuid,
        mode: SandboxMode,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        conn.execute(
            "UPDATE agent_jobs SET job_mode = $2 WHERE id = $1",
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
            .query_opt("SELECT job_mode FROM agent_jobs WHERE id = $1", &[&id])
            .await?;
        row.and_then(|r| r.get::<_, Option<String>>("job_mode"))
            .map(|mode| SandboxMode::try_from(mode.as_str()).map_err(DatabaseError::Serialization))
            .transpose()
    }
}
