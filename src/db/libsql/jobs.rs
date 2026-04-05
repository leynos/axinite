//! Job-related JobStore implementation for LibSqlBackend.

#[path = "jobs/mapping.rs"]
mod mapping;

#[path = "jobs_history.rs"]
mod jobs_history;

use libsql::params;
use uuid::Uuid;

use super::{
    LibSqlBackend, fmt_opt_ts, fmt_ts, get_i64, get_opt_text, get_opt_ts, get_text, get_ts,
    opt_text, opt_text_owned,
};
use crate::context::{ActionRecord, JobContext, JobState};
use crate::db::{EstimationActualsParams, EstimationSnapshotParams, NativeJobStore};
use crate::error::DatabaseError;
use crate::history::{AgentJobRecord, AgentJobSummary, LlmCallRecord};

use chrono::Utc;

const UPSERT_AGENT_JOB_SQL: &str = r#"
                INSERT INTO agent_jobs (
                    id, conversation_id, title, description, category, status, source,
                    user_id,
                    budget_amount, budget_token, bid_amount, estimated_cost, estimated_time_secs,
                    actual_cost, repair_attempts, transitions, metadata, user_timezone,
                    max_tokens, total_tokens_used, created_at, started_at, completed_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)
                ON CONFLICT (id) DO UPDATE SET
                    title = excluded.title,
                    description = excluded.description,
                    category = excluded.category,
                    status = excluded.status,
                    user_id = excluded.user_id,
                    estimated_cost = excluded.estimated_cost,
                    estimated_time_secs = excluded.estimated_time_secs,
                    actual_cost = excluded.actual_cost,
                    repair_attempts = excluded.repair_attempts,
                    transitions = excluded.transitions,
                    metadata = excluded.metadata,
                    user_timezone = excluded.user_timezone,
                    max_tokens = excluded.max_tokens,
                    total_tokens_used = excluded.total_tokens_used,
                    started_at = excluded.started_at,
                    completed_at = excluded.completed_at
                WHERE agent_jobs.source = 'direct'
                "#;

fn checked_duration_seconds(duration: std::time::Duration) -> Result<i64, DatabaseError> {
    i64::try_from(duration.as_secs()).map_err(|error| {
        DatabaseError::Serialization(format!(
            "estimated_duration exceeds i64 range: {} ({error})",
            duration.as_secs()
        ))
    })
}

fn checked_u32_to_i64(value: u32) -> Result<i64, DatabaseError> {
    Ok(i64::from(value))
}

fn checked_u64_to_i64(value: u64, field: &str) -> Result<i64, DatabaseError> {
    i64::try_from(value).map_err(|error| {
        DatabaseError::Serialization(format!("{field} exceeds i64 range: {value} ({error})"))
    })
}

impl LibSqlBackend {
    async fn upsert_agent_job(
        &self,
        conn: &libsql::Connection,
        ctx: &JobContext,
    ) -> Result<(), DatabaseError> {
        let status = ctx.state.to_string();
        let estimated_time_secs = ctx
            .estimated_duration
            .map(checked_duration_seconds)
            .transpose()?;
        let repair_attempts = checked_u32_to_i64(ctx.repair_attempts)?;
        let max_tokens = checked_u64_to_i64(ctx.max_tokens, "max_tokens")?;
        let total_tokens_used = checked_u64_to_i64(ctx.total_tokens_used, "total_tokens_used")?;
        let transitions = serde_json::to_string(&ctx.transitions)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        conn.execute(
            UPSERT_AGENT_JOB_SQL,
            params![
                ctx.job_id.to_string(),
                opt_text_owned(ctx.conversation_id.map(|id| id.to_string())),
                ctx.title.as_str(),
                ctx.description.as_str(),
                opt_text(ctx.category.as_deref()),
                status,
                "direct",
                ctx.user_id.as_str(),
                opt_text_owned(ctx.budget.map(|d| d.to_string())),
                opt_text(ctx.budget_token.as_deref()),
                opt_text_owned(ctx.bid_amount.map(|d| d.to_string())),
                opt_text_owned(ctx.estimated_cost.map(|d| d.to_string())),
                estimated_time_secs,
                ctx.actual_cost.to_string(),
                repair_attempts,
                transitions,
                ctx.metadata.to_string(),
                ctx.user_timezone.as_str(),
                max_tokens,
                total_tokens_used,
                fmt_ts(&ctx.created_at),
                fmt_opt_ts(&ctx.started_at),
                fmt_opt_ts(&ctx.completed_at),
            ],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(())
    }
}

impl NativeJobStore for LibSqlBackend {
    async fn save_job(&self, ctx: &JobContext) -> Result<(), DatabaseError> {
        let conn = self.connect().await?;
        self.upsert_agent_job(&conn, ctx).await
    }

    async fn get_job(&self, id: Uuid) -> Result<Option<JobContext>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, conversation_id, title, description, category, status, user_id,
                       budget_amount, budget_token, bid_amount, estimated_cost, estimated_time_secs,
                       actual_cost, repair_attempts, transitions, metadata, user_timezone,
                       max_tokens, total_tokens_used, created_at, started_at, completed_at
                FROM agent_jobs WHERE id = ?1 AND source = 'direct'
                "#,
                params![id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            Some(row) => mapping::row_to_job_context_libsql(&row).map(Some),
            None => Ok(None),
        }
    }

    async fn update_job_status(
        &self,
        id: Uuid,
        status: JobState,
        failure_reason: Option<&str>,
    ) -> Result<(), DatabaseError> {
        let conn = self.connect().await?;
        conn.execute(
            "UPDATE agent_jobs SET status = ?2, failure_reason = ?3 WHERE id = ?1 AND source = 'direct'",
            params![id.to_string(), status.to_string(), opt_text(failure_reason)],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(())
    }

    async fn mark_job_stuck(&self, id: Uuid) -> Result<(), DatabaseError> {
        let conn = self.connect().await?;
        let now = fmt_ts(&Utc::now());
        conn.execute(
            "UPDATE agent_jobs SET status = 'stuck', stuck_since = ?2 WHERE id = ?1 AND source = 'direct'",
            params![id.to_string(), now],
        )
        .await
        .map_err(|e| DatabaseError::Query(e.to_string()))?;
        Ok(())
    }

    async fn get_stuck_jobs(&self) -> Result<Vec<Uuid>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT id FROM agent_jobs WHERE status = 'stuck' AND source = 'direct'",
                (),
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut ids = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            if let Ok(id_str) = row.get::<String>(0)
                && let Ok(id) = id_str.parse()
            {
                ids.push(id);
            }
        }
        Ok(ids)
    }

    async fn list_agent_jobs(&self) -> Result<Vec<AgentJobRecord>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, title, status, user_id, failure_reason,
                       created_at, started_at, completed_at
                FROM agent_jobs WHERE source = 'direct'
                ORDER BY created_at DESC
                "#,
                (),
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut jobs = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            let id_str = get_text(&row, 0);
            let Ok(id) = id_str.parse() else {
                tracing::warn!("Skipping agent job with invalid UUID: {}", id_str);
                continue;
            };
            jobs.push(AgentJobRecord {
                id,
                title: get_text(&row, 1),
                status: get_text(&row, 2),
                user_id: get_text(&row, 3),
                failure_reason: get_opt_text(&row, 4),
                created_at: get_ts(&row, 5),
                started_at: get_opt_ts(&row, 6),
                completed_at: get_opt_ts(&row, 7),
            });
        }
        Ok(jobs)
    }

    async fn get_agent_job_failure_reason(
        &self,
        id: Uuid,
    ) -> Result<Option<String>, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT failure_reason FROM agent_jobs WHERE id = ?1 AND source = 'direct'",
                [id.to_string()],
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        if let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            Ok(get_opt_text(&row, 0))
        } else {
            Ok(None)
        }
    }

    async fn agent_job_summary(&self) -> Result<AgentJobSummary, DatabaseError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT status, COUNT(*) as cnt FROM agent_jobs WHERE source = 'direct' GROUP BY status",
                (),
            )
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?;

        let mut summary = AgentJobSummary::default();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| DatabaseError::Query(e.to_string()))?
        {
            let status = get_text(&row, 0);
            let count = get_i64(&row, 1) as usize;
            summary.add_count(&status, count);
        }
        Ok(summary)
    }

    async fn save_action(&self, job_id: Uuid, action: &ActionRecord) -> Result<(), DatabaseError> {
        jobs_history::save_action(self, job_id, action).await
    }

    async fn get_job_actions(&self, job_id: Uuid) -> Result<Vec<ActionRecord>, DatabaseError> {
        jobs_history::get_job_actions(self, job_id).await
    }

    async fn record_llm_call(&self, record: &LlmCallRecord<'_>) -> Result<Uuid, DatabaseError> {
        jobs_history::record_llm_call(self, record).await
    }

    async fn save_estimation_snapshot(
        &self,
        params: EstimationSnapshotParams<'_>,
    ) -> Result<Uuid, DatabaseError> {
        jobs_history::save_estimation_snapshot(self, params).await
    }

    async fn update_estimation_actuals(
        &self,
        params: EstimationActualsParams,
    ) -> Result<(), DatabaseError> {
        jobs_history::update_estimation_actuals(self, params).await
    }
}
