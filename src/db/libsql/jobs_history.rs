//! LibSQL job-history persistence helpers.
//!
//! This module reads and writes `ActionRecord` rows, LLM call records, and
//! estimation snapshot/actuals for `LibSqlBackend`, using the shared libSQL
//! helpers (`fmt_ts`, `get_i64`, `get_json`, `get_opt_decimal`, `get_opt_text`,
//! `get_text`, `get_ts`) to decode stored values.

use libsql::params;
use uuid::Uuid;

use super::{LibSqlBackend, fmt_ts, opt_text, opt_text_owned};
use crate::context::ActionRecord;
use crate::db::libsql::helpers::{
    get_i64, get_json, get_opt_decimal, get_opt_text, get_text, get_ts,
};
use crate::db::{EstimationActualsParams, EstimationSnapshotParams};
use crate::error::DatabaseError;
use crate::history::LlmCallRecord;

fn parse_action_id(raw: &str) -> Result<Uuid, DatabaseError> {
    raw.parse().map_err(|error| {
        DatabaseError::Serialization(format!("invalid job_actions.id '{raw}': {error}"))
    })
}

fn parse_non_negative_u32(value: i64, field: &str) -> Result<u32, DatabaseError> {
    if value < 0 {
        return Err(DatabaseError::Serialization(format!(
            "{field} must be non-negative: {value}"
        )));
    }

    u32::try_from(value).map_err(|error| {
        DatabaseError::Serialization(format!("{field} exceeds u32 range: {value} ({error})"))
    })
}

fn parse_duration_millis(value: i64) -> Result<std::time::Duration, DatabaseError> {
    if value < 0 {
        return Err(DatabaseError::Serialization(format!(
            "job action duration_ms must be non-negative: {value}"
        )));
    }

    Ok(std::time::Duration::from_millis(
        u64::try_from(value).map_err(|error| {
            DatabaseError::Serialization(format!(
                "job action duration_ms exceeds u64 range: {value} ({error})"
            ))
        })?,
    ))
}

pub(super) async fn save_action(
    backend: &LibSqlBackend,
    job_id: Uuid,
    action: &ActionRecord,
) -> Result<(), DatabaseError> {
    let conn = backend.connect().await?;
    let duration_ms = i64::try_from(action.duration.as_millis()).map_err(|error| {
        DatabaseError::Serialization(format!(
            "job action duration_ms exceeds i64 range: {} ({error})",
            action.duration.as_millis()
        ))
    })?;
    let warnings_json = serde_json::to_string(&action.sanitization_warnings)
        .map_err(|e| DatabaseError::Serialization(e.to_string()))?;

    conn.execute(
        r#"
            INSERT INTO job_actions (
                id, job_id, sequence_num, tool_name, input, output_raw, output_sanitized,
                sanitization_warnings, cost, duration_ms, success, error_message, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
        params![
            action.id.to_string(),
            job_id.to_string(),
            action.sequence as i64,
            action.tool_name.as_str(),
            action.input.to_string(),
            opt_text(action.output_raw.as_deref()),
            opt_text_owned(action.output_sanitized.as_ref().map(|v| v.to_string())),
            warnings_json,
            opt_text_owned(action.cost.map(|d| d.to_string())),
            duration_ms,
            action.success as i64,
            opt_text(action.error.as_deref()),
            fmt_ts(&action.executed_at),
        ],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(())
}

pub(super) async fn get_job_actions(
    backend: &LibSqlBackend,
    job_id: Uuid,
) -> Result<Vec<ActionRecord>, DatabaseError> {
    let conn = backend.connect().await?;
    let mut rows = conn
        .query(
            r#"
            SELECT id, sequence_num, tool_name, input, output_raw, output_sanitized,
                   sanitization_warnings, cost, duration_ms, success, error_message, created_at
            FROM job_actions WHERE job_id = ?1 ORDER BY sequence_num
            "#,
            params![job_id.to_string()],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;

    let mut actions = Vec::new();
    while let Some(row) = rows
        .next()
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?
    {
        let id_raw = get_text(&row, 0);
        let sequence = parse_non_negative_u32(get_i64(&row, 1), "job action sequence_num")?;
        let warnings_raw = get_text(&row, 6);
        let warnings: Vec<String> = serde_json::from_str(&warnings_raw).map_err(|error| {
            DatabaseError::Serialization(format!(
                "invalid job_actions.sanitization_warnings '{warnings_raw}': {error}"
            ))
        })?;
        let output_sanitized = get_opt_text(&row, 5)
            .map(|value| {
                serde_json::from_str(&value).map_err(|error| {
                    DatabaseError::Serialization(format!(
                        "invalid job_actions.output_sanitized '{value}': {error}"
                    ))
                })
            })
            .transpose()?;
        let duration = parse_duration_millis(get_i64(&row, 8))?;

        actions.push(ActionRecord {
            id: parse_action_id(&id_raw)?,
            sequence,
            tool_name: get_text(&row, 2),
            input: get_json(&row, 3),
            output_raw: get_opt_text(&row, 4),
            output_sanitized,
            sanitization_warnings: warnings,
            cost: get_opt_decimal(&row, 7),
            duration,
            success: get_i64(&row, 9) != 0,
            error: get_opt_text(&row, 10),
            executed_at: get_ts(&row, 11),
        });
    }
    Ok(actions)
}

pub(super) async fn record_llm_call(
    backend: &LibSqlBackend,
    record: &LlmCallRecord<'_>,
) -> Result<Uuid, DatabaseError> {
    let conn = backend.connect().await?;
    let id = Uuid::new_v4();
    conn.execute(
        r#"
        INSERT INTO llm_calls (id, job_id, conversation_id, provider, model, input_tokens, output_tokens, cost, purpose)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        params![
            id.to_string(),
            opt_text_owned(record.job_id.map(|id| id.to_string())),
            opt_text_owned(record.conversation_id.map(|id| id.to_string())),
            record.provider,
            record.model,
            i64::from(record.input_tokens),
            i64::from(record.output_tokens),
            record.cost.to_string(),
            opt_text(record.purpose),
        ],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(id)
}

pub(super) async fn save_estimation_snapshot(
    backend: &LibSqlBackend,
    params: EstimationSnapshotParams<'_>,
) -> Result<Uuid, DatabaseError> {
    let EstimationSnapshotParams {
        job_id,
        category,
        tool_names,
        estimated_cost,
        estimated_time_secs,
        estimated_value,
    } = params;
    let conn = backend.connect().await?;
    let id = Uuid::new_v4();
    let tools_json = serde_json::to_string(tool_names)
        .map_err(|e| DatabaseError::Serialization(e.to_string()))?;

    conn.execute(
        r#"
        INSERT INTO estimation_snapshots (id, job_id, category, tool_names, estimated_cost, estimated_time_secs, estimated_value)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            id.to_string(),
            job_id.to_string(),
            category,
            tools_json,
            estimated_cost.to_string(),
            estimated_time_secs as i64,
            estimated_value.to_string(),
        ],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(id)
}

pub(super) async fn update_estimation_actuals(
    backend: &LibSqlBackend,
    params: EstimationActualsParams,
) -> Result<(), DatabaseError> {
    let EstimationActualsParams {
        id,
        actual_cost,
        actual_time_secs,
        actual_value,
    } = params;
    let conn = backend.connect().await?;
    conn.execute(
        "UPDATE estimation_snapshots SET actual_cost = ?2, actual_time_secs = ?3, actual_value = ?4 WHERE id = ?1",
        params![
            id.to_string(),
            actual_cost.to_string(),
            actual_time_secs as i64,
            actual_value.map(|d| d.to_string()),
        ],
    )
    .await
    .map_err(|e| DatabaseError::Query(e.to_string()))?;
    Ok(())
}
