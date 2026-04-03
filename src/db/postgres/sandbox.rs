//! SandboxStore implementation for PostgreSQL backend.

use uuid::Uuid;

use crate::db::{
    NativeSandboxStore, SandboxEventType, SandboxJobStatusUpdate, SandboxMode, UserId,
};
use crate::error::DatabaseError;
use crate::history::{JobEventRecord, SandboxJobRecord, SandboxJobSummary};

use super::PgBackend;

impl NativeSandboxStore for PgBackend {
    crate::delegate_async! {
        to store;
        async fn save_sandbox_job(&self, job: &SandboxJobRecord) -> Result<(), DatabaseError>;
        async fn get_sandbox_job(&self, id: Uuid) -> Result<Option<SandboxJobRecord>, DatabaseError>;
        async fn list_sandbox_jobs(&self) -> Result<Vec<SandboxJobRecord>, DatabaseError>;
    }

    async fn update_sandbox_job_status(
        &self,
        params: SandboxJobStatusUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        let SandboxJobStatusUpdate {
            id,
            status,
            success,
            message,
            started_at,
            completed_at,
        } = params;
        self.store
            .update_sandbox_job_status(SandboxJobStatusUpdate {
                id,
                status,
                success,
                message,
                started_at,
                completed_at,
            })
            .await
    }

    crate::delegate_async! {
        to store;
        async fn cleanup_stale_sandbox_jobs(&self) -> Result<u64, DatabaseError>;
        async fn sandbox_job_summary(&self) -> Result<SandboxJobSummary, DatabaseError>;
        async fn list_sandbox_jobs_for_user(&self, user_id: UserId) -> Result<Vec<SandboxJobRecord>, DatabaseError>;
        async fn sandbox_job_summary_for_user(&self, user_id: UserId) -> Result<SandboxJobSummary, DatabaseError>;
        async fn sandbox_job_belongs_to_user(&self, job_id: Uuid, user_id: UserId) -> Result<bool, DatabaseError>;
        async fn update_sandbox_job_mode(&self, id: Uuid, mode: SandboxMode) -> Result<(), DatabaseError>;
        async fn get_sandbox_job_mode(&self, id: Uuid) -> Result<Option<SandboxMode>, DatabaseError>;
        async fn save_job_event(&self, job_id: Uuid, event_type: SandboxEventType, data: &serde_json::Value) -> Result<(), DatabaseError>;
        async fn list_job_events(&self, job_id: Uuid, before_id: Option<i64>, limit: Option<i64>) -> Result<Vec<JobEventRecord>, DatabaseError>;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_sandbox_job_status_update_destructuring() {
        // This test verifies that the SandboxJobStatusUpdate struct is correctly
        // destructured and all fields are passed through to the underlying store method.
        // This is a compile-time check - if the struct changes and we miss a field,
        // this will fail to compile.

        let now = Utc::now();
        let update = SandboxJobStatusUpdate {
            id: Uuid::new_v4(),
            status: "completed",
            success: Some(true),
            message: Some("Test message"),
            started_at: Some(now),
            completed_at: Some(now),
        };

        // Destructure to ensure all fields are present
        let SandboxJobStatusUpdate {
            id,
            status,
            success,
            message,
            started_at,
            completed_at,
        } = update;

        // Verify fields are correctly extracted
        assert!(success.expect("expected `success` to be Some(true)"));
        assert_eq!(
            message.expect("expected `message` to be Some"),
            "Test message"
        );
        assert_eq!(status, "completed");
        assert!(started_at.is_some());
        assert!(completed_at.is_some());

        // This pattern ensures we don't accidentally miss fields when updating
        // the update_sandbox_job_status implementation
        let _ = (id, status, success, message, started_at, completed_at);
    }

    /// Behavioral tests for NativeSandboxStore on PgBackend.
    /// These verify field pass-through and method delegation work correctly.
    #[cfg(feature = "postgres")]
    mod behavioral {
        use super::*;
        use crate::testing::try_test_pg_db;
        use rstest::{fixture, rstest};

        #[fixture]
        async fn db() -> Option<PgBackend> {
            try_test_pg_db().await
        }

        #[fixture]
        fn pending_job() -> SandboxJobRecord {
            SandboxJobRecord {
                id: Uuid::new_v4(),
                task: "test_task".to_string(),
                status: "pending".to_string(),
                user_id: "test_user".to_string(),
                project_dir: "/tmp/test".to_string(),
                success: None,
                failure_reason: None,
                created_at: Utc::now(),
                started_at: None,
                completed_at: None,
                credential_grants_json: "[]".to_string(),
            }
        }

        #[rstest]
        #[tokio::test]
        async fn test_update_sandbox_job_status_field_passthrough(
            #[future] db: Option<PgBackend>,
            pending_job: SandboxJobRecord,
        ) {
            let Some(db) = db.await else { return };
            let job_id = pending_job.id;

            db.save_sandbox_job(&pending_job)
                .await
                .expect("failed to save test job");

            // Prepare full status update with all fields populated
            let started = Utc::now();
            let completed = Utc::now();
            let update = SandboxJobStatusUpdate {
                id: job_id,
                status: "completed",
                success: Some(true),
                message: Some("All tests passed"),
                started_at: Some(started),
                completed_at: Some(completed),
            };

            // Update through the trait method
            db.update_sandbox_job_status(update)
                .await
                .expect("update_sandbox_job_status should succeed");

            // Verify all fields were passed through correctly
            let retrieved = db
                .get_sandbox_job(job_id)
                .await
                .expect("get_sandbox_job should succeed")
                .expect("job should exist");

            assert_eq!(retrieved.status, "completed");
            assert_eq!(retrieved.success, Some(true));
            assert_eq!(
                retrieved.failure_reason,
                Some("All tests passed".to_string())
            );
            assert!(retrieved.started_at.is_some());
            assert!(retrieved.completed_at.is_some());
        }

        #[rstest]
        #[tokio::test]
        async fn test_list_sandbox_jobs_for_user(
            #[future] db: Option<PgBackend>,
            pending_job: SandboxJobRecord,
        ) {
            let Some(db) = db.await else { return };

            // Generate unique user IDs to avoid cross-run collisions
            let user1_id = format!("user1-{}", Uuid::new_v4());
            let user2_id = format!("user2-{}", Uuid::new_v4());
            let user3_id = format!("user3-{}", Uuid::new_v4());

            // Create jobs for two different users
            let mut user1_job = pending_job.clone();
            user1_job.user_id = user1_id.clone();
            user1_job.task = "task1".to_string();
            user1_job.project_dir = "/tmp/user1".to_string();

            let mut user2_job = pending_job.clone();
            user2_job.id = Uuid::new_v4();
            user2_job.user_id = user2_id.clone();
            user2_job.task = "task2".to_string();
            user2_job.project_dir = "/tmp/user2".to_string();

            db.save_sandbox_job(&user1_job)
                .await
                .expect("failed to save user1 job");
            db.save_sandbox_job(&user2_job)
                .await
                .expect("failed to save user2 job");

            // List jobs for user1
            let user1_jobs = db
                .list_sandbox_jobs_for_user(UserId::from(user1_id.clone()))
                .await
                .expect("list_sandbox_jobs_for_user should succeed");

            assert_eq!(user1_jobs.len(), 1);
            assert_eq!(user1_jobs[0].user_id, user1_id);
            assert_eq!(user1_jobs[0].id, user1_job.id);

            // List jobs for user2
            let user2_jobs = db
                .list_sandbox_jobs_for_user(UserId::from(user2_id.clone()))
                .await
                .expect("list_sandbox_jobs_for_user should succeed");

            assert_eq!(user2_jobs.len(), 1);
            assert_eq!(user2_jobs[0].user_id, user2_id);
            assert_eq!(user2_jobs[0].id, user2_job.id);

            // Verify user3 has no jobs
            let user3_jobs = db
                .list_sandbox_jobs_for_user(UserId::from(user3_id.clone()))
                .await
                .expect("list_sandbox_jobs_for_user should succeed");

            assert!(user3_jobs.is_empty());
        }

        #[rstest]
        #[tokio::test]
        async fn test_sandbox_job_summary_for_user(
            #[future] db: Option<PgBackend>,
            pending_job: SandboxJobRecord,
        ) {
            let Some(db) = db.await else { return };

            // Generate unique user IDs to avoid cross-run collisions
            let user_id = format!("summary-user-{}", Uuid::new_v4());
            let other_user_id = format!("summary-other-{}", Uuid::new_v4());

            // Create multiple jobs with different statuses for one user
            let mut pending = pending_job.clone();
            pending.user_id = user_id.clone();
            pending.task = "pending_task".to_string();
            pending.project_dir = "/tmp/pending".to_string();

            let mut completed_job = pending_job.clone();
            completed_job.id = Uuid::new_v4();
            completed_job.user_id = user_id.clone();
            completed_job.task = "completed_task".to_string();
            completed_job.status = "completed".to_string();
            completed_job.project_dir = "/tmp/completed".to_string();
            completed_job.success = Some(true);
            completed_job.failure_reason = Some("Success".to_string());
            completed_job.started_at = Some(Utc::now());
            completed_job.completed_at = Some(Utc::now());

            let mut other_user_job = pending_job.clone();
            other_user_job.id = Uuid::new_v4();
            other_user_job.user_id = other_user_id;
            other_user_job.task = "other_task".to_string();
            other_user_job.status = "pending".to_string();
            other_user_job.project_dir = "/tmp/other".to_string();

            db.save_sandbox_job(&pending)
                .await
                .expect("failed to save pending job");
            db.save_sandbox_job(&completed_job)
                .await
                .expect("failed to save completed job");
            db.save_sandbox_job(&other_user_job)
                .await
                .expect("failed to save other user job");

            // Get summary for user
            let summary = db
                .sandbox_job_summary_for_user(UserId::from(user_id.clone()))
                .await
                .expect("sandbox_job_summary_for_user should succeed");

            assert_eq!(summary.total, 2);
        }

        #[rstest]
        #[case(SandboxMode::Worker)]
        #[case(SandboxMode::ClaudeCode)]
        #[tokio::test]
        async fn test_sandbox_job_mode_roundtrip(
            #[future] db: Option<PgBackend>,
            pending_job: SandboxJobRecord,
            #[case] mode: SandboxMode,
        ) {
            let Some(db) = db.await else { return };
            let job_id = pending_job.id;

            db.save_sandbox_job(&pending_job)
                .await
                .expect("failed to save sandbox job");

            db.update_sandbox_job_mode(job_id, mode)
                .await
                .expect("update_sandbox_job_mode should succeed");

            let stored_mode = db
                .get_sandbox_job_mode(job_id)
                .await
                .expect("get_sandbox_job_mode should succeed");
            assert_eq!(stored_mode, Some(mode));

            let conn = db.store.conn().await.expect("failed to get connection");
            conn.execute("DELETE FROM agent_jobs WHERE id = $1", &[&job_id])
                .await
                .expect("failed to clean up sandbox job");
        }

        #[rstest]
        #[tokio::test]
        async fn test_save_job_event_roundtrip(
            #[future] db: Option<PgBackend>,
            pending_job: SandboxJobRecord,
        ) {
            let Some(db) = db.await else { return };
            let job_id = pending_job.id;
            let event_type = SandboxEventType::from("stdout");
            let data = serde_json::json!({
                "message": "hello from postgres sandbox adapter",
            });

            db.save_sandbox_job(&pending_job)
                .await
                .expect("failed to save sandbox job");

            db.save_job_event(job_id, event_type.clone(), &data)
                .await
                .expect("save_job_event should succeed");

            let events = db
                .list_job_events(job_id, None, None)
                .await
                .expect("list_job_events should succeed");
            let stored_event = events
                .into_iter()
                .find(|event| event.job_id == job_id && event.event_type == event_type.as_str())
                .expect("expected persisted sandbox event");

            assert_eq!(stored_event.data, data);

            let conn = db.store.conn().await.expect("failed to get connection");
            conn.execute("DELETE FROM job_events WHERE job_id = $1", &[&job_id])
                .await
                .expect("failed to clean up job events");
            conn.execute("DELETE FROM agent_jobs WHERE id = $1", &[&job_id])
                .await
                .expect("failed to clean up sandbox job");
        }

        #[rstest]
        #[tokio::test]
        async fn test_sandbox_job_belongs_to_user(
            #[future] db: Option<PgBackend>,
            pending_job: SandboxJobRecord,
        ) {
            let Some(db) = db.await else { return };

            // Generate unique user IDs to avoid cross-run collisions
            let owner_id = format!("owner-{}", Uuid::new_v4());
            let other_id = format!("other-{}", Uuid::new_v4());

            let mut job = pending_job.clone();
            job.user_id = owner_id.clone();
            job.task = "ownership_test".to_string();
            job.project_dir = "/tmp/owner".to_string();
            let job_id = job.id;

            db.save_sandbox_job(&job).await.expect("failed to save job");

            // Test ownership check
            let belongs = db
                .sandbox_job_belongs_to_user(job_id, UserId::from(owner_id.clone()))
                .await
                .expect("sandbox_job_belongs_to_user should succeed");

            assert!(belongs, "job should belong to owner");

            // Test non-ownership
            let not_belongs = db
                .sandbox_job_belongs_to_user(job_id, UserId::from(other_id.clone()))
                .await
                .expect("sandbox_job_belongs_to_user should succeed");

            assert!(!not_belongs, "job should not belong to other_user");
        }
    }
}
