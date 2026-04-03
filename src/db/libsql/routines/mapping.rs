//! Row-decoding helpers for libSQL routine persistence.
//!
//! `ROUTINE_COLUMNS` and `ROUTINE_RUN_COLUMNS` define the strict positional
//! column order expected by the mapping functions below. Any schema or SELECT
//! list changes must update these constants and the corresponding parsing logic
//! together so row decoding remains correct.

use crate::agent::routine::{
    NotifyConfig, Routine, RoutineAction, RoutineGuardrails, RoutineRun, RunStatus, Trigger,
};
use crate::db::libsql::helpers::{get_i64, get_json, get_opt_text, get_opt_ts, get_text, get_ts};
use crate::error::DatabaseError;

fn parse_uuid_field(raw: &str, field: &str) -> Result<uuid::Uuid, DatabaseError> {
    raw.parse()
        .map_err(|error| DatabaseError::Serialization(format!("invalid {field} '{raw}': {error}")))
}

fn parse_optional_uuid_field(
    raw: Option<String>,
    field: &str,
) -> Result<Option<uuid::Uuid>, DatabaseError> {
    raw.map(|value| {
        value.parse().map_err(|error| {
            DatabaseError::Serialization(format!("invalid {field} '{value}': {error}"))
        })
    })
    .transpose()
}

pub(crate) const ROUTINE_COLUMNS: &str = "\
    id, name, description, user_id, enabled, \
    trigger_type, trigger_config, action_type, action_config, \
    cooldown_secs, max_concurrent, dedup_window_secs, \
    notify_channel, notify_user, notify_on_success, notify_on_failure, notify_on_attention, \
    state, last_run_at, next_fire_at, run_count, consecutive_failures, \
    created_at, updated_at";

pub(crate) const ROUTINE_RUN_COLUMNS: &str = "\
    id, routine_id, trigger_type, trigger_detail, started_at, \
    status, completed_at, result_summary, tokens_used, job_id, created_at";

pub(crate) fn row_to_routine_libsql(row: &libsql::Row) -> Result<Routine, DatabaseError> {
    let trigger_type = get_text(row, 5);
    let trigger_config = get_json(row, 6);
    let action_type = get_text(row, 7);
    let action_config = get_json(row, 8);
    let cooldown_secs = get_i64(row, 9);
    let max_concurrent = get_i64(row, 10);
    let dedup_window_secs: Option<i64> = row.get::<i64>(11).ok();

    let trigger = Trigger::from_db(&trigger_type, trigger_config)
        .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
    let action = RoutineAction::from_db(&action_type, action_config)
        .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
    let id_raw = get_text(row, 0);

    Ok(Routine {
        id: parse_uuid_field(&id_raw, "routines.id")?,
        name: get_text(row, 1),
        description: get_text(row, 2),
        user_id: get_text(row, 3),
        enabled: get_i64(row, 4) != 0,
        trigger,
        action,
        guardrails: RoutineGuardrails {
            cooldown: std::time::Duration::from_secs(cooldown_secs as u64),
            max_concurrent: max_concurrent as u32,
            dedup_window: dedup_window_secs.map(|s| std::time::Duration::from_secs(s as u64)),
        },
        notify: NotifyConfig {
            channel: get_opt_text(row, 12),
            user: get_text(row, 13),
            on_success: get_i64(row, 14) != 0,
            on_failure: get_i64(row, 15) != 0,
            on_attention: get_i64(row, 16) != 0,
        },
        state: get_json(row, 17),
        last_run_at: get_opt_ts(row, 18),
        next_fire_at: get_opt_ts(row, 19),
        run_count: get_i64(row, 20) as u64,
        consecutive_failures: get_i64(row, 21) as u32,
        created_at: get_ts(row, 22),
        updated_at: get_ts(row, 23),
    })
}

pub(crate) fn row_to_routine_run_libsql(row: &libsql::Row) -> Result<RoutineRun, DatabaseError> {
    let status_str = get_text(row, 5);
    let status: RunStatus = status_str
        .parse()
        .map_err(|e: crate::error::RoutineError| DatabaseError::Serialization(e.to_string()))?;
    let id_raw = get_text(row, 0);
    let routine_id_raw = get_text(row, 1);

    Ok(RoutineRun {
        id: parse_uuid_field(&id_raw, "routine_runs.id")?,
        routine_id: parse_uuid_field(&routine_id_raw, "routine_runs.routine_id")?,
        trigger_type: get_text(row, 2),
        trigger_detail: get_opt_text(row, 3),
        started_at: get_ts(row, 4),
        completed_at: get_opt_ts(row, 6),
        status,
        result_summary: get_opt_text(row, 7),
        tokens_used: row.get::<i64>(8).ok().map(|v| v as i32),
        job_id: parse_optional_uuid_field(get_opt_text(row, 9), "routine_runs.job_id")?,
        created_at: get_ts(row, 10),
    })
}
