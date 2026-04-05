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

    /// Load job events for a job, ordered by ascending id.
    ///
    /// `before_id` is an exclusive cursor (`id < before_id`) and, when
    /// present, must be greater than zero. `limit`, when present, must also be
    /// greater than zero.
    pub async fn list_job_events(
        &self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<JobEventRecord>, DatabaseError> {
        if let Some(limit) = limit
            && limit <= 0
        {
            return Err(DatabaseError::Query(
                "list_job_events limit must be greater than 0".to_string(),
            ));
        }
        if let Some(cursor) = before_id
            && cursor <= 0
        {
            return Err(DatabaseError::Query(
                "list_job_events before_id must be greater than 0".to_string(),
            ));
        }

        let conn = self.conn().await?;
        let rows = Self::fetch_job_event_rows(&conn, job_id, before_id, limit).await?;
        Ok(rows.iter().map(Self::job_event_record_from_row).collect())
    }

    async fn fetch_job_event_rows(
        conn: &deadpool_postgres::Object,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<tokio_postgres::Row>, DatabaseError> {
        match (before_id, limit) {
            (Some(before_id), Some(n)) => Ok(conn
                .query(
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
                .await?),
            (Some(before_id), None) => Ok(conn
                .query(
                    r#"
                    SELECT id, job_id, event_type, data, created_at
                    FROM job_events
                    WHERE job_id = $1 AND id < $2
                    ORDER BY id ASC
                    "#,
                    &[&job_id, &before_id],
                )
                .await?),
            (None, Some(n)) => Ok(conn
                .query(
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
                .await?),
            (None, None) => Ok(conn
                .query(
                    r#"
                    SELECT id, job_id, event_type, data, created_at
                    FROM job_events
                    WHERE job_id = $1
                    ORDER BY id ASC
                    "#,
                    &[&job_id],
                )
                .await?),
        }
    }

    fn job_event_record_from_row(r: &tokio_postgres::Row) -> JobEventRecord {
        JobEventRecord {
            id: r.get("id"),
            job_id: r.get("job_id"),
            event_type: SandboxEventType::from(r.get::<_, String>("event_type")),
            data: r.get("data"),
            created_at: r.get("created_at"),
        }
    }
}

#[cfg(all(test, feature = "postgres"))]
mod tests {
    use chrono::Utc;
    use rstest::{fixture, rstest};
    use uuid::Uuid;

    use super::*;
    use crate::db::SandboxEventType;
    use crate::history::SandboxJobRecord;
    use crate::testing::postgres::try_test_pg_db;

    #[fixture]
    async fn store() -> Option<Store> {
        let backend = try_test_pg_db().await?;
        Some(Store::from_pool(backend.pool()))
    }

    async fn seed_sandbox_job(store: &Store, job_id: Uuid) {
        let job = SandboxJobRecord {
            id: job_id,
            task: "sandbox event test".to_string(),
            status: "creating".to_string(),
            user_id: crate::db::UserId::from(format!("sandbox-event-{}", Uuid::new_v4())),
            project_dir: "/tmp/sandbox-event".to_string(),
            success: None,
            failure_reason: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            credential_grants_json: "{}".to_string(),
        };
        store
            .save_sandbox_job(&job)
            .await
            .expect("sandbox job should seed");
    }

    async fn cleanup_job(store: &Store, job_id: Uuid) {
        let conn = store.conn().await.expect("connection should succeed");
        conn.execute("DELETE FROM job_events WHERE job_id = $1", &[&job_id])
            .await
            .expect("job events should delete");
        conn.execute("DELETE FROM agent_jobs WHERE id = $1", &[&job_id])
            .await
            .expect("sandbox job should delete");
    }

    #[rstest]
    #[case(Some(0), "list_job_events limit must be greater than 0")]
    #[case(Some(-1), "list_job_events limit must be greater than 0")]
    #[tokio::test]
    async fn list_job_events_with_non_positive_limit_errors(
        #[future] store: Option<Store>,
        #[case] limit: Option<i64>,
        #[case] expected: &str,
    ) {
        let Some(store) = store.await else { return };
        let result = store.list_job_events(Uuid::new_v4(), None, limit).await;
        assert!(matches!(
            result,
            Err(DatabaseError::Query(message)) if message == expected
        ));
    }

    #[rstest]
    #[case(Some(0), "list_job_events before_id must be greater than 0")]
    #[case(Some(-1), "list_job_events before_id must be greater than 0")]
    #[tokio::test]
    async fn list_job_events_with_non_positive_before_id_errors(
        #[future] store: Option<Store>,
        #[case] before_id: Option<i64>,
        #[case] expected: &str,
    ) {
        let Some(store) = store.await else { return };
        let result = store.list_job_events(Uuid::new_v4(), before_id, None).await;
        assert!(matches!(
            result,
            Err(DatabaseError::Query(message)) if message == expected
        ));
    }

    #[rstest]
    #[tokio::test]
    async fn list_job_events_with_valid_inputs_succeeds(#[future] store: Option<Store>) {
        let Some(store) = store.await else { return };
        let job_id = Uuid::new_v4();
        seed_sandbox_job(&store, job_id).await;

        for message in ["one", "two", "three"] {
            store
                .save_job_event(
                    job_id,
                    SandboxEventType::from("stdout"),
                    &serde_json::json!({ "message": message }),
                )
                .await
                .expect("job event should save");
        }

        let all = store
            .list_job_events(job_id, None, None)
            .await
            .expect("unbounded list should succeed");
        assert_eq!(all.len(), 3);
        assert!(all.windows(2).all(|pair| pair[0].id < pair[1].id));

        let limited = store
            .list_job_events(job_id, None, Some(2))
            .await
            .expect("limited list should succeed");
        assert_eq!(limited.len(), 2);
        assert_eq!(limited[0].data["message"], "two");
        assert_eq!(limited[1].data["message"], "three");

        let before = store
            .list_job_events(job_id, Some(all[2].id), None)
            .await
            .expect("cursor list should succeed");
        assert_eq!(before.len(), 2);
        assert!(before.iter().all(|event| event.id < all[2].id));
        assert!(before.windows(2).all(|pair| pair[0].id < pair[1].id));

        cleanup_job(&store, job_id).await;
    }
}
