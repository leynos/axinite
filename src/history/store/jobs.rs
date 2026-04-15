//! Agent-job persistence types and helpers.

use chrono::{DateTime, Utc};
use uuid::Uuid;

#[cfg(feature = "postgres")]
#[path = "jobs/mapping.rs"]
mod mapping;

#[cfg(feature = "postgres")]
use self::mapping::{JobUpsertFields, row_to_job_context};
#[cfg(feature = "postgres")]
use super::Store;
#[cfg(feature = "postgres")]
use crate::context::{JobContext, JobState};
#[cfg(feature = "postgres")]
use crate::db::TerminalJobPersistence;
#[cfg(feature = "postgres")]
use crate::error::DatabaseError;

#[cfg(feature = "postgres")]
const UPSERT_AGENT_JOB_SQL: &str = r#"
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
            "#;

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
        let f = JobUpsertFields::from_context(ctx)?;
        let repair_attempts = i32::try_from(ctx.repair_attempts).map_err(|error| {
            DatabaseError::Serialization(format!(
                "repair_attempts exceeds i32 range: {} ({error})",
                ctx.repair_attempts
            ))
        })?;

        conn.execute(
            UPSERT_AGENT_JOB_SQL,
            &[
                &ctx.job_id,
                &ctx.conversation_id,
                &ctx.title,
                &ctx.description,
                &ctx.category,
                &f.status_str,
                &"direct",
                &ctx.user_id,
                &ctx.budget,
                &ctx.budget_token,
                &ctx.bid_amount,
                &ctx.estimated_cost,
                &f.estimated_time_secs_i32,
                &ctx.actual_cost,
                &repair_attempts,
                &f.transitions_json,
                &f.metadata_json,
                &ctx.user_timezone,
                &f.max_tokens_i64,
                &f.total_tokens_used_i64,
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

        row.map(|row| row_to_job_context(&row)).transpose()
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

    /// Persist a terminal result event and terminal status in one transaction.
    pub async fn persist_terminal_result_and_status(
        &self,
        params: TerminalJobPersistence<'_>,
    ) -> Result<(), DatabaseError> {
        let TerminalJobPersistence {
            job_id,
            status,
            failure_reason,
            event_type,
            event_data,
        } = params;
        let mut conn = self.conn().await?;
        let tx = conn.transaction().await?;
        let status_str = status.to_string();

        tx.execute(
            r#"
            INSERT INTO job_events (job_id, event_type, data)
            VALUES ($1, $2, $3)
            "#,
            &[&job_id, &event_type.as_str(), event_data],
        )
        .await?;
        let rows_affected = tx
            .execute(
            "UPDATE agent_jobs SET status = $2, failure_reason = $3 WHERE id = $1 AND source = 'direct'",
            &[&job_id, &status_str, &failure_reason],
        )
        .await?;
        if rows_affected != 1 {
            tx.rollback().await?;
            return Err(DatabaseError::NotFound {
                entity: "agent_job".to_string(),
                id: job_id.to_string(),
            });
        }
        tx.commit().await?;
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
            let count = usize::try_from(row.get::<_, i64>("cnt")).map_err(|error| {
                DatabaseError::Serialization(format!(
                    "agent job summary count exceeds usize range: {error}"
                ))
            })?;
            summary.add_count(&status, count);
        }
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "postgres")]
    use crate::context::StateTransition;
    #[cfg(feature = "postgres")]
    use crate::db::TerminalJobPersistence;
    #[cfg(feature = "postgres")]
    use crate::db::postgres::PgBackend;
    #[cfg(feature = "postgres")]
    use crate::testing::postgres::try_test_pg_db;
    #[cfg(feature = "postgres")]
    use rstest::rstest;
    #[cfg(feature = "postgres")]
    use serde_json::json;

    #[cfg(feature = "postgres")]
    enum RollbackScenario {
        UnknownJob,
        NonDirectJob,
    }

    #[cfg(feature = "postgres")]
    async fn prepare_job_for_rollback(
        backend: &PgBackend,
        store: &Store,
        scenario: RollbackScenario,
    ) -> Result<(Uuid, Option<JobContext>), Box<dyn std::error::Error>> {
        match scenario {
            RollbackScenario::UnknownJob => Ok((Uuid::new_v4(), None)),
            RollbackScenario::NonDirectJob => {
                let ctx = JobContext::with_user("test-user", "sandbox-like job", "rollback check");
                let job_id = ctx.job_id;
                store.save_job(&ctx).await?;

                let conn = backend.pool().get().await?;
                conn.execute(
                    "UPDATE agent_jobs SET source = 'sandbox' WHERE id = $1",
                    &[&job_id],
                )
                .await?;

                Ok((job_id, Some(ctx)))
            }
        }
    }

    /// Regression test: save_job must persist user-owned and context fields.
    /// Requires a running PostgreSQL instance (integration tier).
    #[cfg(feature = "postgres")]
    #[rstest]
    #[tokio::test]
    async fn test_save_job_persists_user_id() {
        use crate::context::JobContext;

        let Some(backend) = try_test_pg_db()
            .await
            .expect("unexpected Postgres test setup error")
        else {
            return;
        };
        let store = Store::from_pool(backend.pool());

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

        let conn = backend.pool().get().await.expect("pool get should succeed");
        conn.execute("DELETE FROM agent_jobs WHERE id = $1", &[&ctx.job_id])
            .await
            .expect("delete agent_jobs should succeed");
    }

    #[test]
    fn agent_job_summary_accumulates_status_buckets() {
        let mut summary = AgentJobSummary::default();
        summary.add_count("pending", 1);
        summary.add_count("submitted", 6);
        summary.add_count("accepted", 7);
        summary.add_count("in_progress", 2);
        summary.add_count("completed", 3);
        summary.add_count("failed", 4);
        summary.add_count("stuck", 5);
        summary.add_count("cancelled", 8);

        assert_eq!(summary.total, 36);
        assert_eq!(summary.pending, 1);
        assert_eq!(summary.in_progress, 2);
        assert_eq!(summary.completed, 16);
        assert_eq!(summary.failed, 12);
        assert_eq!(summary.stuck, 5);
    }

    #[cfg(feature = "postgres")]
    #[rstest]
    #[case(RollbackScenario::UnknownJob)]
    #[case(RollbackScenario::NonDirectJob)]
    #[tokio::test]
    async fn persist_terminal_result_and_status_rolls_back_on_invalid_job(
        #[case] scenario: RollbackScenario,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(backend) = try_test_pg_db().await? else {
            return Ok(());
        };
        let store = Store::from_pool(backend.pool());
        let (job_id, saved_ctx) =
            prepare_job_for_rollback(&backend, &store, scenario).await?;

        let result = store
            .persist_terminal_result_and_status(TerminalJobPersistence {
                job_id,
                status: JobState::Failed,
                failure_reason: Some("terminal rollback regression"),
                event_type: crate::db::SandboxEventType::from("result"),
                event_data: &json!({"status": "failed"}),
            })
            .await;
        assert!(result.is_err(), "invalid terminal job write should fail");

        let conn = backend.pool().get().await?;
        let count: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM job_events WHERE job_id = $1",
                &[&job_id],
            )
            .await?
            .get(0);
        assert_eq!(count, 0, "rollback should remove inserted job_events rows");
        if let Some(ctx) = saved_ctx {
            conn.execute("DELETE FROM agent_jobs WHERE id = $1", &[&ctx.job_id])
                .await?;
        }
        Ok(())
    }
}
