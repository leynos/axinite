//! Routine-run persistence: recording, completing, listing, and linking runs.

use chrono::Utc;
use uuid::Uuid;

use crate::agent::routine::RoutineRun;
use crate::db::RoutineRunCompletion;
use crate::error::DatabaseError;
use crate::history::store::Store;

use super::mapping::row_to_routine_run;

impl Store {
    /// Record a routine run starting.
    pub async fn create_routine_run(&self, run: &RoutineRun) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        let status = run.status.to_string();
        conn.execute(
            r#"
            INSERT INTO routine_runs (
                id, routine_id, trigger_type, trigger_detail,
                started_at, status, job_id
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            "#,
            &[
                &run.id,
                &run.routine_id,
                &run.trigger_type,
                &run.trigger_detail,
                &run.started_at,
                &status,
                &run.job_id,
            ],
        )
        .await?;
        Ok(())
    }

    /// Complete a routine run.
    pub async fn complete_routine_run(
        &self,
        params: RoutineRunCompletion<'_>,
    ) -> Result<(), DatabaseError> {
        let RoutineRunCompletion {
            id,
            status,
            result_summary,
            tokens_used,
        } = params;
        let conn = self.conn().await?;
        let status_str = status.to_string();
        let now = Utc::now();
        let affected = conn
            .execute(
                r#"
                UPDATE routine_runs SET
                    completed_at = $2, status = $3,
                    result_summary = $4, tokens_used = $5
                WHERE id = $1
                "#,
                &[&id, &now, &status_str, &result_summary, &tokens_used],
            )
            .await?;
        if affected == 0 {
            return Err(DatabaseError::NotFound {
                entity: "routine run".to_string(),
                id: id.to_string(),
            });
        }
        Ok(())
    }

    /// List recent runs for a routine.
    pub async fn list_routine_runs(
        &self,
        routine_id: Uuid,
        limit: i64,
    ) -> Result<Vec<RoutineRun>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = conn
            .query(
                r#"
                SELECT * FROM routine_runs
                WHERE routine_id = $1
                ORDER BY started_at DESC
                LIMIT $2
                "#,
                &[&routine_id, &limit],
            )
            .await?;
        rows.iter().map(row_to_routine_run).collect()
    }

    /// Count currently running runs for a routine.
    pub async fn count_running_routine_runs(&self, routine_id: Uuid) -> Result<i64, DatabaseError> {
        let conn = self.conn().await?;
        let row = conn
            .query_one(
                "SELECT COUNT(*) as cnt FROM routine_runs WHERE routine_id = $1 AND status = 'running'",
                &[&routine_id],
            )
            .await?;
        Ok(row.get("cnt"))
    }

    /// Link a routine run to a dispatched job.
    pub async fn link_routine_run_to_job(
        &self,
        run_id: Uuid,
        job_id: Uuid,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        let affected = conn
            .execute(
                "UPDATE routine_runs SET job_id = $1 WHERE id = $2",
                &[&job_id, &run_id],
            )
            .await?;
        if affected == 0 {
            return Err(DatabaseError::NotFound {
                entity: "routine run".to_string(),
                id: run_id.to_string(),
            });
        }
        Ok(())
    }
}
