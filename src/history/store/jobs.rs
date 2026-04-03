//! Agent-job persistence types and helpers.

use chrono::{DateTime, Utc};
#[cfg(feature = "postgres")]
use rust_decimal::Decimal;
use uuid::Uuid;

#[cfg(feature = "postgres")]
use super::{Store, parse_job_state};
#[cfg(feature = "postgres")]
use crate::context::{JobContext, JobState, StateTransition};
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;

/// Lightweight record for agent (non-sandbox) jobs, used by the web Jobs tab.
#[derive(Debug, Clone)]
pub struct AgentJobRecord {
    pub id: Uuid,
    pub title: String,
    pub status: String,
    pub user_id: String,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub failure_reason: Option<String>,
}

/// Summary counts for agent (non-sandbox) jobs.
#[derive(Debug, Clone, Default)]
pub struct AgentJobSummary {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub completed: usize,
    pub failed: usize,
    pub stuck: usize,
}

impl AgentJobSummary {
    /// Accumulate a status/count pair into the summary buckets.
    pub fn add_count(&mut self, status: &str, count: usize) {
        self.total += count;
        match status {
            "pending" => self.pending += count,
            "in_progress" => self.in_progress += count,
            "completed" | "submitted" | "accepted" => self.completed += count,
            "failed" | "cancelled" => self.failed += count,
            "stuck" => self.stuck += count,
            _ => {}
        }
    }
}

#[cfg(feature = "postgres")]
impl Store {
    /// Save a job context to the database.
    pub async fn save_job(&self, ctx: &JobContext) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;

        let status = ctx.state.to_string();
        let estimated_time_secs = ctx.estimated_duration.map(|d| d.as_secs() as i32);
        let transitions = serde_json::to_value(&ctx.transitions)
            .map_err(|e| DatabaseError::Serialization(e.to_string()))?;

        conn.execute(
            r#"
            INSERT INTO agent_jobs (
                id, conversation_id, title, description, category, status, source,
                user_id,
                budget_amount, budget_token, bid_amount, estimated_cost, estimated_time_secs,
                actual_cost, repair_attempts, transitions, metadata, user_timezone,
                max_tokens, total_tokens_used, created_at, started_at, completed_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23)
            ON CONFLICT (id) DO UPDATE SET
                title = EXCLUDED.title,
                description = EXCLUDED.description,
                category = EXCLUDED.category,
                status = EXCLUDED.status,
                user_id = EXCLUDED.user_id,
                estimated_cost = EXCLUDED.estimated_cost,
                estimated_time_secs = EXCLUDED.estimated_time_secs,
                actual_cost = EXCLUDED.actual_cost,
                repair_attempts = EXCLUDED.repair_attempts,
                transitions = EXCLUDED.transitions,
                metadata = EXCLUDED.metadata,
                user_timezone = EXCLUDED.user_timezone,
                max_tokens = EXCLUDED.max_tokens,
                total_tokens_used = EXCLUDED.total_tokens_used,
                started_at = EXCLUDED.started_at,
                completed_at = EXCLUDED.completed_at
            "#,
            &[
                &ctx.job_id,
                &ctx.conversation_id,
                &ctx.title,
                &ctx.description,
                &ctx.category,
                &status,
                &"direct",
                &ctx.user_id,
                &ctx.budget,
                &ctx.budget_token,
                &ctx.bid_amount,
                &ctx.estimated_cost,
                &estimated_time_secs,
                &ctx.actual_cost,
                &(ctx.repair_attempts as i32),
                &transitions,
                &ctx.metadata,
                &ctx.user_timezone,
                &(ctx.max_tokens as i64),
                &(ctx.total_tokens_used as i64),
                &ctx.created_at,
                &ctx.started_at,
                &ctx.completed_at,
            ],
        )
        .await?;

        Ok(())
    }

    /// Get a job by ID.
    pub async fn get_job(&self, id: Uuid) -> Result<Option<JobContext>, DatabaseError> {
        let conn = self.conn().await?;

        let row = conn
            .query_opt(
                r#"
                SELECT id, conversation_id, title, description, category, status, user_id,
                       budget_amount, budget_token, bid_amount, estimated_cost, estimated_time_secs,
                       actual_cost, repair_attempts, transitions, metadata, user_timezone,
                       max_tokens, total_tokens_used, created_at, started_at, completed_at
                FROM agent_jobs WHERE id = $1 AND source = 'direct'
                "#,
                &[&id],
            )
            .await?;

        match row {
            Some(row) => {
                let status_str: String = row.get("status");
                let state = parse_job_state(&status_str)?;
                let estimated_time_secs: Option<i32> = row.get("estimated_time_secs");
                let transitions_json: serde_json::Value = row.get("transitions");
                let transitions: Vec<StateTransition> = serde_json::from_value(transitions_json)
                    .map_err(|e| DatabaseError::Serialization(e.to_string()))?;
                let metadata: serde_json::Value = row.get("metadata");

                Ok(Some(JobContext {
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
                        .map(|s| std::time::Duration::from_secs(s as u64)),
                    actual_cost: row
                        .get::<_, Option<Decimal>>("actual_cost")
                        .unwrap_or_default(),
                    repair_attempts: row.get::<_, i32>("repair_attempts") as u32,
                    transitions,
                    metadata,
                    created_at: row.get("created_at"),
                    started_at: row.get("started_at"),
                    completed_at: row.get("completed_at"),
                    max_tokens: row.get::<_, Option<i64>>("max_tokens").unwrap_or(0) as u64,
                    total_tokens_used: row.get::<_, Option<i64>>("total_tokens_used").unwrap_or(0)
                        as u64,
                    extra_env: std::sync::Arc::new(std::collections::HashMap::new()),
                    http_interceptor: None,
                    tool_output_stash: std::sync::Arc::new(tokio::sync::RwLock::new(
                        std::collections::HashMap::new(),
                    )),
                    user_timezone: row
                        .get::<_, Option<String>>("user_timezone")
                        .unwrap_or_else(|| "UTC".to_string()),
                }))
            }
            None => Ok(None),
        }
    }

    /// Update job status.
    pub async fn update_job_status(
        &self,
        id: Uuid,
        status: JobState,
        failure_reason: Option<&str>,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        let status_str = status.to_string();

        conn.execute(
            "UPDATE agent_jobs SET status = $2, failure_reason = $3 WHERE id = $1 AND source = 'direct'",
            &[&id, &status_str, &failure_reason],
        )
        .await?;

        Ok(())
    }

    /// Mark job as stuck.
    pub async fn mark_job_stuck(&self, id: Uuid) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;

        conn.execute(
            "UPDATE agent_jobs SET status = 'stuck', stuck_since = NOW() WHERE id = $1 AND source = 'direct'",
            &[&id],
        )
        .await?;

        Ok(())
    }

    /// Get stuck jobs.
    pub async fn get_stuck_jobs(&self) -> Result<Vec<Uuid>, DatabaseError> {
        let conn = self.conn().await?;

        let rows = conn
            .query(
                "SELECT id FROM agent_jobs WHERE status = 'stuck' AND source = 'direct'",
                &[],
            )
            .await?;

        Ok(rows.iter().map(|r| r.get("id")).collect())
    }

    /// List all agent (non-sandbox) jobs, most recent first.
    pub async fn list_agent_jobs(&self) -> Result<Vec<AgentJobRecord>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                r#"
                SELECT id, title, status, user_id, failure_reason,
                       created_at, started_at, completed_at
                FROM agent_jobs WHERE source = 'direct'
                ORDER BY created_at DESC
                "#,
                &[],
            )
            .await?;

        Ok(rows
            .iter()
            .map(|r| AgentJobRecord {
                id: r.get("id"),
                title: r.get("title"),
                status: r.get("status"),
                user_id: r.get::<_, Option<String>>("user_id").unwrap_or_default(),
                created_at: r.get("created_at"),
                started_at: r.get("started_at"),
                completed_at: r.get("completed_at"),
                failure_reason: r.get("failure_reason"),
            })
            .collect())
    }

    /// Get the failure reason for a single agent job.
    pub async fn get_agent_job_failure_reason(
        &self,
        id: Uuid,
    ) -> Result<Option<String>, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_opt(
                "SELECT failure_reason FROM agent_jobs WHERE id = $1 AND source = 'direct'",
                &[&id],
            )
            .await?;
        Ok(row.and_then(|r| r.get::<_, Option<String>>("failure_reason")))
    }

    /// Summary counts for agent (non-sandbox) jobs.
    pub async fn agent_job_summary(&self) -> Result<AgentJobSummary, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                "SELECT status, COUNT(*) as cnt FROM agent_jobs WHERE source = 'direct' GROUP BY status",
                &[],
            )
            .await?;

        let mut summary = AgentJobSummary::default();
        for row in &rows {
            let status: String = row.get("status");
            let count: i64 = row.get("cnt");
            summary.add_count(&status, count as usize);
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test: save_job must persist user-owned and context fields.
    /// Requires a running PostgreSQL instance (integration tier).
    #[cfg(feature = "postgres")]
    #[tokio::test]
    #[ignore]
    async fn test_save_job_persists_user_id() {
        use crate::config::Config;
        use crate::context::JobContext;

        let _ = dotenvy::dotenv();
        let config = Config::from_env().await.expect("Failed to load config");
        let store = Store::new(&config.database)
            .await
            .expect("Failed to connect to database");
        store
            .run_migrations()
            .await
            .expect("Failed to run migrations");

        let ctx = JobContext::with_user("test-user-42", "PG user_id test", "regression test");
        let mut ctx = ctx.with_timezone("Europe/London");
        ctx.metadata = serde_json::json!({ "mode": "regression" });
        ctx.transitions.push(StateTransition {
            from: JobState::Pending,
            to: JobState::InProgress,
            timestamp: Utc::now(),
            reason: Some("test".to_string()),
        });
        store.save_job(&ctx).await.expect("save_job should succeed");

        let loaded = store
            .get_job(ctx.job_id)
            .await
            .expect("get_job should succeed")
            .expect("job should exist");
        assert_eq!(loaded.user_id, "test-user-42");
        assert_eq!(loaded.user_timezone, "Europe/London");
        assert_eq!(loaded.metadata, ctx.metadata);
        assert_eq!(loaded.transitions.len(), 1);

        let conn = store.conn().await.unwrap();
        conn.execute("DELETE FROM agent_jobs WHERE id = $1", &[&ctx.job_id])
            .await
            .unwrap();
    }

    #[test]
    fn agent_job_summary_accumulates_status_buckets() {
        let mut summary = AgentJobSummary::default();
        summary.add_count("pending", 1);
        summary.add_count("in_progress", 2);
        summary.add_count("completed", 3);
        summary.add_count("failed", 4);
        summary.add_count("stuck", 5);

        assert_eq!(summary.total, 15);
        assert_eq!(summary.pending, 1);
        assert_eq!(summary.in_progress, 2);
        assert_eq!(summary.completed, 3);
        assert_eq!(summary.failed, 4);
        assert_eq!(summary.stuck, 5);
    }
}
