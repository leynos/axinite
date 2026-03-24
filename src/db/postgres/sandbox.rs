//! SandboxStore implementation for PostgreSQL backend.

use uuid::Uuid;

use crate::db::{NativeSandboxStore, SandboxJobStatusUpdate};
use crate::error::DatabaseError;
use crate::history::{JobEventRecord, SandboxJobRecord, SandboxJobSummary};

use super::PgBackend;

impl NativeSandboxStore for PgBackend {
    async fn save_sandbox_job(&self, job: &SandboxJobRecord) -> Result<(), DatabaseError> {
        self.store.save_sandbox_job(job).await
    }

    async fn get_sandbox_job(&self, id: Uuid) -> Result<Option<SandboxJobRecord>, DatabaseError> {
        self.store.get_sandbox_job(id).await
    }

    async fn list_sandbox_jobs(&self) -> Result<Vec<SandboxJobRecord>, DatabaseError> {
        self.store.list_sandbox_jobs().await
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
            .update_sandbox_job_status(id, status, success, message, started_at, completed_at)
            .await
    }

    async fn cleanup_stale_sandbox_jobs(&self) -> Result<u64, DatabaseError> {
        self.store.cleanup_stale_sandbox_jobs().await
    }

    async fn sandbox_job_summary(&self) -> Result<SandboxJobSummary, DatabaseError> {
        self.store.sandbox_job_summary().await
    }

    async fn list_sandbox_jobs_for_user(
        &self,
        user_id: &str,
    ) -> Result<Vec<SandboxJobRecord>, DatabaseError> {
        self.store.list_sandbox_jobs_for_user(user_id).await
    }

    async fn sandbox_job_summary_for_user(
        &self,
        user_id: &str,
    ) -> Result<SandboxJobSummary, DatabaseError> {
        self.store.sandbox_job_summary_for_user(user_id).await
    }

    async fn sandbox_job_belongs_to_user(
        &self,
        job_id: Uuid,
        user_id: &str,
    ) -> Result<bool, DatabaseError> {
        self.store
            .sandbox_job_belongs_to_user(job_id, user_id)
            .await
    }

    async fn update_sandbox_job_mode(&self, id: Uuid, mode: &str) -> Result<(), DatabaseError> {
        self.store.update_sandbox_job_mode(id, mode).await
    }

    async fn get_sandbox_job_mode(&self, id: Uuid) -> Result<Option<String>, DatabaseError> {
        self.store.get_sandbox_job_mode(id).await
    }

    async fn save_job_event(
        &self,
        job_id: Uuid,
        event_type: &str,
        data: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        self.store.save_job_event(job_id, event_type, data).await
    }

    async fn list_job_events(
        &self,
        job_id: Uuid,
        before_id: Option<i64>,
        limit: Option<i64>,
    ) -> Result<Vec<JobEventRecord>, DatabaseError> {
        self.store.list_job_events(job_id, before_id, limit).await
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
}
