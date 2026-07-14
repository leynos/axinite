//! SandboxStore implementation for PostgreSQL backend.

use uuid::Uuid;

#[cfg(test)]
use crate::db::SandboxJobStatus;
use crate::db::{
    NativeSandboxStore, SandboxEventType, SandboxJobStatusUpdate, SandboxMode, UserId,
};
use crate::error::DatabaseError;
use crate::history::{JobEventRecord, SandboxJobRecord, SandboxJobSummary};

use super::PgBackend;

impl NativeSandboxStore for PgBackend {
    crate::db::delegate_async! {
        to store;
        async fn save_sandbox_job(&self, job: &SandboxJobRecord) -> Result<(), DatabaseError>;
        async fn get_sandbox_job(&self, id: Uuid) -> Result<Option<SandboxJobRecord>, DatabaseError>;
        async fn list_sandbox_jobs(&self) -> Result<Vec<SandboxJobRecord>, DatabaseError>;
    }

    async fn update_sandbox_job_status(
        &self,
        params: SandboxJobStatusUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        self.store.update_sandbox_job_status(params).await
    }

    crate::db::delegate_async! {
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
mod tests;
