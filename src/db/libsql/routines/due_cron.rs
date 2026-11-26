//! Claiming of due cron routines for LibSQL.

use chrono::Utc;
use libsql::params;

use super::super::{fmt_ts, get_text};
use super::mapping;
use crate::agent::routine::Routine;
use crate::error::DatabaseError;

/// Load every enabled cron routine due at `now` and push its `next_fire_at`
/// out to `lease_until`, so a concurrent poller does not claim it too.
///
/// The caller owns the surrounding transaction.
pub(super) async fn claim_due_cron_routines(
    conn: &libsql::Connection,
    now: &str,
    lease_until: &str,
) -> Result<Vec<Routine>, DatabaseError> {
    let mut routines = Vec::new();
    for due_id in select_due_cron_ids(conn, now).await? {
        routines.push(claim_due_routine(conn, &due_id, lease_until).await?);
    }
    Ok(routines)
}

/// IDs of the enabled cron routines whose `next_fire_at` is at or before `now`.
async fn select_due_cron_ids(
    conn: &libsql::Connection,
    now: &str,
) -> Result<Vec<String>, DatabaseError> {
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
    Ok(due_ids)
}

/// Load one due routine and lease it by moving its `next_fire_at` to `lease_until`.
async fn claim_due_routine(
    conn: &libsql::Connection,
    due_id: &str,
    lease_until: &str,
) -> Result<Routine, DatabaseError> {
    let mut rows = conn
        .query(
            &format!(
                "SELECT {} FROM routines WHERE id = ?1",
                mapping::ROUTINE_COLUMNS
            ),
            params![due_id],
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
        params![routine.id.to_string(), lease_until, fmt_ts(&Utc::now())],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(routine)
}
