//! Row-to-domain mapping helpers for routine persistence.

use crate::agent::routine::{
    NotifyConfig, Routine, RoutineAction, RoutineGuardrails, RoutineRun, RunStatus, Trigger,
};
use crate::error::{DatabaseError, RoutineError};

fn parse_non_negative_u64(value: i64, field: &str) -> Result<u64, DatabaseError> {
    if value < 0 {
        return Err(DatabaseError::Serialization(format!(
            "{field} must be non-negative: {value}"
        )));
    }

    u64::try_from(value).map_err(|error| {
        DatabaseError::Serialization(format!("{field} exceeds u64 range: {value} ({error})"))
    })
}

fn parse_non_negative_u32(value: i32, field: &str) -> Result<u32, DatabaseError> {
    if value < 0 {
        return Err(DatabaseError::Serialization(format!(
            "{field} must be non-negative: {value}"
        )));
    }

    u32::try_from(value).map_err(|error| {
        DatabaseError::Serialization(format!("{field} exceeds u32 range: {value} ({error})"))
    })
}

pub(crate) fn row_to_routine(row: &tokio_postgres::Row) -> Result<Routine, DatabaseError> {
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
    let cooldown = parse_non_negative_u64(i64::from(cooldown_secs), "routines.cooldown_secs")?;
    let max_concurrent = parse_non_negative_u32(max_concurrent, "routines.max_concurrent")?;
    let dedup_window = dedup_window_secs
        .map(|seconds| parse_non_negative_u64(i64::from(seconds), "routines.dedup_window_secs"))
        .transpose()?;
    let run_count = parse_non_negative_u64(row.get::<_, i64>("run_count"), "routines.run_count")?;
    let consecutive_failures = parse_non_negative_u32(
        row.get::<_, i32>("consecutive_failures"),
        "routines.consecutive_failures",
    )?;

    Ok(Routine {
        id: row.get("id"),
        name: row.get("name"),
        description: row.get("description"),
        user_id: row.get("user_id"),
        enabled: row.get("enabled"),
        trigger,
        action,
        guardrails: RoutineGuardrails {
            cooldown: std::time::Duration::from_secs(cooldown),
            max_concurrent,
            dedup_window: dedup_window.map(std::time::Duration::from_secs),
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
        run_count,
        consecutive_failures,
        state: row.get("state"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

pub(crate) fn row_to_routine_run(row: &tokio_postgres::Row) -> Result<RoutineRun, DatabaseError> {
    let status_str: String = row.get("status");
    let status: RunStatus = status_str
        .parse()
        .map_err(|e: RoutineError| DatabaseError::Serialization(e.to_string()))?;

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
