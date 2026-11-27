//! Atomic insert-or-update of sandbox job rows in `agent_jobs`.

use crate::db::SandboxMode;
use crate::error::DatabaseError;
use crate::history::{SandboxJobRecord, Store};

impl Store {
    /// Insert or update a sandbox job in `agent_jobs`.
    pub async fn save_sandbox_job(&self, job: &SandboxJobRecord) -> Result<(), DatabaseError> {
        self.save_sandbox_job_with_mode(job, None).await
    }

    /// Insert or update a sandbox job and its execution mode in one statement.
    ///
    /// `mode` is `None` for the default [`SandboxMode::Worker`]; an existing
    /// row keeps its stored mode when `None` is passed.
    pub async fn save_sandbox_job_with_mode(
        &self,
        job: &SandboxJobRecord,
        mode: Option<SandboxMode>,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        let mode_str = mode.map(SandboxMode::as_str);
        let rows_affected = conn
            .execute(
                r#"
            INSERT INTO agent_jobs (
                id, title, description, status, source, user_id, project_dir,
                success, failure_reason, created_at, started_at, completed_at,
                job_mode
            ) VALUES (
                $1, $2, $3, $4, 'sandbox', $5, $6, $7, $8, $9, $10, $11,
                COALESCE($12, 'worker')
            )
            ON CONFLICT (id) DO UPDATE SET
                title = EXCLUDED.title,
                description = EXCLUDED.description,
                user_id = EXCLUDED.user_id,
                project_dir = EXCLUDED.project_dir,
                status = EXCLUDED.status,
                success = EXCLUDED.success,
                failure_reason = EXCLUDED.failure_reason,
                started_at = EXCLUDED.started_at,
                completed_at = EXCLUDED.completed_at,
                job_mode = COALESCE($12, agent_jobs.job_mode)
            WHERE agent_jobs.source = 'sandbox'
            "#,
                &[
                    &job.id,
                    &job.task,
                    &job.credential_grants_json,
                    &job.status,
                    &job.user_id.as_str(),
                    &job.project_dir,
                    &job.success,
                    &job.failure_reason,
                    &job.created_at,
                    &job.started_at,
                    &job.completed_at,
                    &mode_str,
                ],
            )
            .await?;
        if rows_affected == 0 {
            return Err(DatabaseError::Query(format!(
                "sandbox job {} conflicts with a non-sandbox agent_jobs row",
                job.id
            )));
        }
        Ok(())
    }
}
