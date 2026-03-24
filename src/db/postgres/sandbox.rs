//! SandboxStore implementation for PostgreSQL backend.

use uuid::Uuid;

use crate::db::{NativeSandboxStore, SandboxJobStatusUpdate};
use crate::error::DatabaseError;
use crate::history::{SandboxJobRecord, SandboxJobSummary};

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

    async fn mark_sandbox_job_started(&self, id: Uuid) -> Result<(), DatabaseError> {
        self.store.mark_sandbox_job_started(id).await
    }

    async fn mark_sandbox_job_completed(
        &self,
        id: Uuid,
        success: bool,
        message: Option<&str>,
    ) -> Result<(), DatabaseError> {
        self.store
            .mark_sandbox_job_completed(id, success, message)
            .await
    }

    async fn delete_sandbox_job(&self, id: Uuid) -> Result<(), DatabaseError> {
        self.store.delete_sandbox_job(id).await
    }
}
