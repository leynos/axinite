//! LibSQL routine-run persistence helpers.

use chrono::Utc;
use libsql::params;
use uuid::Uuid;

use super::mapping;
use super::{LibSqlBackend, fmt_ts, opt_text, opt_text_owned};
use crate::agent::routine::RoutineRun;
use crate::db::RoutineRunCompletion;
use crate::error::DatabaseError;

pub(super) async fn create_routine_run(
    backend: &LibSqlBackend,
    run: &RoutineRun,
) -> Result<(), DatabaseError> {
    let conn = backend.connect().await?;
    conn.execute(
        r#"
            INSERT INTO routine_runs (
                id, routine_id, trigger_type, trigger_detail,
                started_at, status, job_id
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        params![
            run.id.to_string(),
            run.routine_id.to_string(),
            run.trigger_type.as_str(),
            opt_text(run.trigger_detail.as_deref()),
            fmt_ts(&run.started_at),
            run.status.to_string(),
            opt_text_owned(run.job_id.map(|id| id.to_string())),
        ],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(())
}

pub(super) async fn complete_routine_run(
    backend: &LibSqlBackend,
    params: RoutineRunCompletion<'_>,
) -> Result<(), DatabaseError> {
    let RoutineRunCompletion {
        id,
        status,
        result_summary,
        tokens_used,
    } = params;
    let conn = backend.connect().await?;
    let now = fmt_ts(&Utc::now());
    conn.execute(
        r#"
            UPDATE routine_runs SET
                completed_at = ?5, status = ?2,
                result_summary = ?3, tokens_used = ?4
            WHERE id = ?1
            "#,
        params![
            id.to_string(),
            status.to_string(),
            opt_text(result_summary),
            tokens_used.map(i64::from),
            now,
        ],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(())
}

pub(super) async fn list_routine_runs(
    backend: &LibSqlBackend,
    routine_id: Uuid,
    limit: i64,
) -> Result<Vec<RoutineRun>, DatabaseError> {
    if limit <= 0 {
        return Err(DatabaseError::Query(
            "list_routine_runs limit must be greater than 0".to_string(),
        ));
    }
    let conn = backend.connect().await?;
    let mut rows = conn
        .query(
            &format!(
                "SELECT {} FROM routine_runs WHERE routine_id = ?1 ORDER BY started_at DESC LIMIT ?2",
                mapping::ROUTINE_RUN_COLUMNS
            ),
            params![routine_id.to_string(), limit],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let mut runs = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?
    {
        runs.push(mapping::row_to_routine_run_libsql(&row)?);
    }
    Ok(runs)
}

pub(super) async fn count_running_routine_runs(
    backend: &LibSqlBackend,
    routine_id: Uuid,
) -> Result<i64, DatabaseError> {
    let conn = backend.connect().await?;
    let mut rows = conn
        .query(
            "SELECT COUNT(*) as cnt FROM routine_runs WHERE routine_id = ?1 AND status = 'running'",
            params![routine_id.to_string()],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

    match rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?
    {
        Some(row) => Ok(super::get_i64(&row, 0)),
        None => Ok(0),
    }
}

pub(super) async fn link_routine_run_to_job(
    backend: &LibSqlBackend,
    run_id: Uuid,
    job_id: Uuid,
) -> Result<(), DatabaseError> {
    let conn = backend.connect().await?;
    conn.execute(
        "UPDATE routine_runs SET job_id = ?1 WHERE id = ?2",
        params![job_id.to_string(), run_id.to_string()],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(())
}
