//! Atomic insert-or-update of sandbox job rows in `agent_jobs`.

use libsql::params;

use crate::db::SandboxMode;
use crate::db::libsql::{LibSqlBackend, fmt_opt_ts, fmt_ts, opt_text};
use crate::error::DatabaseError;
use crate::history::SandboxJobRecord;

/// Insert or update a sandbox job and its execution mode in one statement.
///
/// `mode` is `None` for the default [`SandboxMode::Worker`]; an existing row
/// keeps its stored mode when `None` is passed.
pub(super) async fn save_sandbox_job_with_mode(
    backend: &LibSqlBackend,
    job: &SandboxJobRecord,
    mode: Option<SandboxMode>,
) -> Result<(), DatabaseError> {
    let conn = backend.connect().await?;
    let rows_affected = conn
        .execute(
            r#"
            INSERT INTO agent_jobs (
                id, title, description, status, source, user_id, project_dir,
                success, failure_reason, created_at, started_at, completed_at,
                job_mode
            ) VALUES (
                ?1, ?2, ?3, ?4, 'sandbox', ?5, ?6, ?7, ?8, ?9, ?10, ?11,
                COALESCE(?12, 'worker')
            )
            ON CONFLICT (id) DO UPDATE SET
                title = excluded.title,
                description = excluded.description,
                user_id = excluded.user_id,
                project_dir = excluded.project_dir,
                status = excluded.status,
                success = excluded.success,
                failure_reason = excluded.failure_reason,
                started_at = excluded.started_at,
                completed_at = excluded.completed_at,
                job_mode = COALESCE(?12, agent_jobs.job_mode)
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
                opt_text(mode.map(SandboxMode::as_str)),
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
