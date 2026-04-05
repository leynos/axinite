//! Row-decoding helpers for libSQL direct-agent jobs.

use uuid::Uuid;

use crate::context::{JobContext, StateTransition};
use crate::db::libsql::helpers::{
    get_decimal, get_i64, get_json, get_opt_decimal, get_opt_text, get_opt_ts, get_text, get_ts,
};
use crate::db::libsql::parse_job_state;
use crate::error::DatabaseError;

const JOB_ID_COL: i32 = 0;
const JOB_CONVERSATION_ID_COL: i32 = 1;
const JOB_TITLE_COL: i32 = 2;
const JOB_DESCRIPTION_COL: i32 = 3;
const JOB_CATEGORY_COL: i32 = 4;
const JOB_STATUS_COL: i32 = 5;
const JOB_USER_ID_COL: i32 = 6;
const JOB_BUDGET_AMOUNT_COL: i32 = 7;
const JOB_BUDGET_TOKEN_COL: i32 = 8;
const JOB_BID_AMOUNT_COL: i32 = 9;
const JOB_ESTIMATED_COST_COL: i32 = 10;
const JOB_ESTIMATED_TIME_SECS_COL: i32 = 11;
const JOB_ACTUAL_COST_COL: i32 = 12;
const JOB_REPAIR_ATTEMPTS_COL: i32 = 13;
const JOB_TRANSITIONS_COL: i32 = 14;
const JOB_METADATA_COL: i32 = 15;
const JOB_USER_TIMEZONE_COL: i32 = 16;
const JOB_MAX_TOKENS_COL: i32 = 17;
const JOB_TOTAL_TOKENS_USED_COL: i32 = 18;
const JOB_CREATED_AT_COL: i32 = 19;
const JOB_STARTED_AT_COL: i32 = 20;
const JOB_COMPLETED_AT_COL: i32 = 21;

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

fn parse_optional_non_negative_u64(value: Option<i64>, field: &str) -> Result<u64, DatabaseError> {
    value
        .map(|value| parse_non_negative_u64(value, field))
        .transpose()
        .map(|value| value.unwrap_or(0))
}

fn parse_optional_duration(
    value: Option<i64>,
) -> Result<Option<std::time::Duration>, DatabaseError> {
    value
        .map(|seconds| {
            let seconds = parse_non_negative_u64(seconds, "agent_jobs.estimated_time_secs")?;
            Ok(std::time::Duration::from_secs(seconds))
        })
        .transpose()
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

pub(super) fn row_to_job_context_libsql(row: &libsql::Row) -> Result<JobContext, DatabaseError> {
    let status_str = get_text(row, JOB_STATUS_COL);
    let state = parse_job_state(&status_str);
    let estimated_time_secs: Option<i64> = row.get::<i64>(JOB_ESTIMATED_TIME_SECS_COL).ok();
    let transitions: Vec<StateTransition> =
        serde_json::from_value(get_json(row, JOB_TRANSITIONS_COL))
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
    let metadata = get_json(row, JOB_METADATA_COL);
    let job_id_raw = get_text(row, JOB_ID_COL);
    let job_id = Uuid::parse_str(&job_id_raw).map_err(|e| {
        DatabaseError::Serialization(format!("Invalid agent_jobs.id '{job_id_raw}': {e}"))
    })?;
    let conversation_id = match get_opt_text(row, JOB_CONVERSATION_ID_COL) {
        Some(raw) => Some(Uuid::parse_str(&raw).map_err(|e| {
            DatabaseError::Serialization(format!("Invalid agent_jobs.conversation_id '{raw}': {e}"))
        })?),
        None => None,
    };

    Ok(JobContext {
        job_id,
        state,
        user_id: get_text(row, JOB_USER_ID_COL),
        conversation_id,
        title: get_text(row, JOB_TITLE_COL),
        description: get_text(row, JOB_DESCRIPTION_COL),
        category: get_opt_text(row, JOB_CATEGORY_COL),
        budget: get_opt_decimal(row, JOB_BUDGET_AMOUNT_COL),
        budget_token: get_opt_text(row, JOB_BUDGET_TOKEN_COL),
        bid_amount: get_opt_decimal(row, JOB_BID_AMOUNT_COL),
        estimated_cost: get_opt_decimal(row, JOB_ESTIMATED_COST_COL),
        estimated_duration: parse_optional_duration(estimated_time_secs)?,
        actual_cost: get_decimal(row, JOB_ACTUAL_COST_COL),
        repair_attempts: parse_non_negative_u32(
            get_i64(row, JOB_REPAIR_ATTEMPTS_COL),
            "agent_jobs.repair_attempts",
        )?,
        transitions,
        metadata,
        max_tokens: parse_optional_non_negative_u64(
            row.get::<i64>(JOB_MAX_TOKENS_COL).ok(),
            "agent_jobs.max_tokens",
        )?,
        total_tokens_used: parse_optional_non_negative_u64(
            row.get::<i64>(JOB_TOTAL_TOKENS_USED_COL).ok(),
            "agent_jobs.total_tokens_used",
        )?,
        created_at: get_ts(row, JOB_CREATED_AT_COL),
        started_at: get_opt_ts(row, JOB_STARTED_AT_COL),
        completed_at: get_opt_ts(row, JOB_COMPLETED_AT_COL),
        extra_env: std::sync::Arc::new(std::collections::HashMap::new()),
        http_interceptor: None,
        tool_output_stash: std::sync::Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        user_timezone: get_text(row, JOB_USER_TIMEZONE_COL),
    })
}
