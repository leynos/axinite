//! Sandbox job-event persistence helpers.

use uuid::Uuid;

use super::{JobEventRecord, Store};
use crate::db::SandboxEventType;
use crate::error::DatabaseError;

#[cfg(feature = "postgres")]
impl Store {
    /// Persist a job event (fire-and-forget from orchestrator handler).
    pub async fn save_job_event(
        &self,
        job_id: Uuid,
        event_type: SandboxEventType,
        data: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        let conn = self.conn().await?;
        conn.execute(
            r#"
            INSERT INTO job_events (job_id, event_type, data)
            VALUES ($1, $2, $3)
            "#,
            &[&job_id, &event_type.as_str(), data],
        )
        .await?;
        Ok(())
    }

    /// Load job events for a job, ordered by id.
    pub async fn list_job_events(
        &self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<JobEventRecord>, DatabaseError> {
        let conn = self.conn().await?;
        let rows = match (before_id, limit) {
            (Some(before_id), Some(n)) => {
                conn.query(
                    r#"
                    SELECT id, job_id, event_type, data, created_at
                    FROM (
                        SELECT id, job_id, event_type, data, created_at
                        FROM job_events
                        WHERE job_id = $1 AND id < $2
                        ORDER BY id DESC
                        LIMIT $3
                    ) sub
                    ORDER BY id ASC
                    "#,
                    &[&job_id, &before_id, &n],
                )
                .await?
            }
            (Some(before_id), None) => {
                conn.query(
                    r#"
                    SELECT id, job_id, event_type, data, created_at
                    FROM job_events
                    WHERE job_id = $1 AND id < $2
                    ORDER BY id ASC
                    "#,
                    &[&job_id, &before_id],
                )
                .await?
            }
            (None, Some(n)) => {
                conn.query(
                    r#"
                    SELECT id, job_id, event_type, data, created_at
                    FROM (
                        SELECT id, job_id, event_type, data, created_at
                        FROM job_events
                        WHERE job_id = $1
                        ORDER BY id DESC
                        LIMIT $2
                    ) sub
                    ORDER BY id ASC
                    "#,
                    &[&job_id, &n],
                )
                .await?
            }
            (None, None) => {
                conn.query(
                    r#"
                    SELECT id, job_id, event_type, data, created_at
                    FROM job_events
                    WHERE job_id = $1
                    ORDER BY id ASC
                    "#,
                    &[&job_id],
                )
                .await?
            }
        };
        Ok(rows
            .iter()
            .map(|r| JobEventRecord {
                id: r.get("id"),
                job_id: r.get("job_id"),
                event_type: r.get("event_type"),
                data: r.get("data"),
                created_at: r.get("created_at"),
            })
            .collect())
    }
}
