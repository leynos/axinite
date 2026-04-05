//! Shared row-decoding and field-derivation helpers for agent jobs.

use rust_decimal::Decimal;

use crate::context::{JobContext, StateTransition};
use crate::error::DatabaseError;

use super::super::parse_job_state;

pub(super) struct JobUpsertFields {
    pub(super) status_str: String,
    pub(super) transitions_json: serde_json::Value,
    pub(super) metadata_json: serde_json::Value,
    pub(super) estimated_time_secs_i32: Option<i32>,
    pub(super) max_tokens_i64: i64,
    pub(super) total_tokens_used_i64: i64,
}

impl JobUpsertFields {
    pub(super) fn from_context(ctx: &JobContext) -> Result<Self, DatabaseError> {
        let status_str = ctx.state.to_string();
        let estimated_time_secs_i32 = ctx
            .estimated_duration
            .map(|duration| {
                i32::try_from(duration.as_secs()).map_err(|error| {
                    DatabaseError::Serialization(format!(
                        "estimated_duration exceeds i32 range: {} ({error})",
                        duration.as_secs()
                    ))
                })
            })
            .transpose()?;
        let max_tokens_i64 = i64::try_from(ctx.max_tokens).map_err(|error| {
            DatabaseError::Serialization(format!(
                "max_tokens exceeds i64 range: {} ({error})",
                ctx.max_tokens
            ))
        })?;
        let total_tokens_used_i64 = i64::try_from(ctx.total_tokens_used).map_err(|error| {
            DatabaseError::Serialization(format!(
                "total_tokens_used exceeds i64 range: {} ({error})",
                ctx.total_tokens_used
            ))
        })?;
        let transitions_json = serde_json::to_value(&ctx.transitions)
            .map_err(|error| DatabaseError::Serialization(error.to_string()))?;

        Ok(Self {
            status_str,
            transitions_json,
            metadata_json: ctx.metadata.clone(),
            estimated_time_secs_i32,
            max_tokens_i64,
            total_tokens_used_i64,
        })
    }
}

pub(super) fn parse_non_negative_u64_field(
    value: Option<i64>,
    field: &str,
) -> Result<u64, DatabaseError> {
    match value {
        Some(value) if value < 0 => Err(DatabaseError::Serialization(format!(
            "{field} must be non-negative: {value}"
        ))),
        Some(value) => u64::try_from(value).map_err(|error| {
            DatabaseError::Serialization(format!("{field} exceeds u64 range: {value} ({error})"))
        }),
        None => Ok(0),
    }
}

pub(super) fn parse_non_negative_u32_field(value: i32, field: &str) -> Result<u32, DatabaseError> {
    if value < 0 {
        return Err(DatabaseError::Serialization(format!(
            "{field} must be non-negative: {value}"
        )));
    }

    u32::try_from(value).map_err(|error| {
        DatabaseError::Serialization(format!("{field} exceeds u32 range: {value} ({error})"))
    })
}

pub(super) fn row_to_job_context(row: &tokio_postgres::Row) -> Result<JobContext, DatabaseError> {
    let status_str: String = row.get("status");
    let state = parse_job_state(&status_str)?;
    let estimated_time_secs: Option<i32> = row.get("estimated_time_secs");
    let transitions_json: serde_json::Value = row.get("transitions");
    let transitions: Vec<StateTransition> = serde_json::from_value(transitions_json)
        .map_err(|error| DatabaseError::Serialization(error.to_string()))?;
    let metadata: serde_json::Value = row.get("metadata");

    Ok(JobContext {
        job_id: row.get("id"),
        state,
        user_id: row.get::<_, String>("user_id"),
        conversation_id: row.get("conversation_id"),
        title: row.get("title"),
        description: row.get("description"),
        category: row.get("category"),
        budget: row.get("budget_amount"),
        budget_token: row.get("budget_token"),
        bid_amount: row.get("bid_amount"),
        estimated_cost: row.get("estimated_cost"),
        estimated_duration: estimated_time_secs
            .map(|seconds| -> Result<std::time::Duration, DatabaseError> {
                let seconds = parse_non_negative_u64_field(
                    Some(i64::from(seconds)),
                    "agent_jobs.estimated_time_secs",
                )?;
                Ok(std::time::Duration::from_secs(seconds))
            })
            .transpose()?,
        actual_cost: row
            .get::<_, Option<Decimal>>("actual_cost")
            .unwrap_or_default(),
        repair_attempts: parse_non_negative_u32_field(
            row.get::<_, i32>("repair_attempts"),
            "agent_jobs.repair_attempts",
        )?,
        transitions,
        metadata,
        created_at: row.get("created_at"),
        started_at: row.get("started_at"),
        completed_at: row.get("completed_at"),
        max_tokens: parse_non_negative_u64_field(
            row.get::<_, Option<i64>>("max_tokens"),
            "agent_jobs.max_tokens",
        )?,
        total_tokens_used: parse_non_negative_u64_field(
            row.get::<_, Option<i64>>("total_tokens_used"),
            "agent_jobs.total_tokens_used",
        )?,
        extra_env: std::sync::Arc::new(std::collections::HashMap::new()),
        http_interceptor: None,
        tool_output_stash: std::sync::Arc::new(tokio::sync::RwLock::new(
            std::collections::HashMap::new(),
        )),
        user_timezone: row
            .get::<_, Option<String>>("user_timezone")
            .unwrap_or_else(|| "UTC".to_string()),
    })
}
