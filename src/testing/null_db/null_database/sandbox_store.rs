//! Null implementation of NativeSandboxStore for NullDatabase.

use uuid::Uuid;

use crate::db::{SandboxEventType, SandboxJobStatusUpdate, SandboxMode, UserId};
use crate::error::DatabaseError;
use crate::history::{JobEventRecord, SandboxJobRecord, SandboxJobSummary};

use super::NullDatabase;

impl crate::db::NativeSandboxStore for NullDatabase {
    async fn save_sandbox_job(&self, _job: &SandboxJobRecord) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_sandbox_job(&self, _id: Uuid) -> Result<Option<SandboxJobRecord>, DatabaseError> {
        Ok(None)
    }

    async fn list_sandbox_jobs(&self) -> Result<Vec<SandboxJobRecord>, DatabaseError> {
        Ok(vec![])
    }

    async fn update_sandbox_job_status(
        &self,
        _params: SandboxJobStatusUpdate<'_>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn cleanup_stale_sandbox_jobs(&self) -> Result<u64, DatabaseError> {
        Ok(0)
    }

    async fn sandbox_job_summary(&self) -> Result<SandboxJobSummary, DatabaseError> {
        Ok(SandboxJobSummary::default())
    }

    async fn list_sandbox_jobs_for_user(
        &self,
        _user_id: UserId,
    ) -> Result<Vec<SandboxJobRecord>, DatabaseError> {
        Ok(vec![])
    }

    async fn sandbox_job_summary_for_user(
        &self,
        _user_id: UserId,
    ) -> Result<SandboxJobSummary, DatabaseError> {
        Ok(SandboxJobSummary::default())
    }

    async fn sandbox_job_belongs_to_user(
        &self,
        _job_id: Uuid,
        _user_id: UserId,
    ) -> Result<bool, DatabaseError> {
        Ok(false)
    }

    async fn update_sandbox_job_mode(
        &self,
        _id: Uuid,
        _mode: SandboxMode,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_sandbox_job_mode(&self, _id: Uuid) -> Result<Option<SandboxMode>, DatabaseError> {
        Ok(None)
    }

    async fn save_job_event(
        &self,
        _job_id: Uuid,
        _event_type: SandboxEventType,
        _data: &serde_json::Value,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn list_job_events(
        &self,
        _job_id: Uuid,
        _before_id: Option<i64>,
        _limit: Option<i64>,
    ) -> Result<Vec<JobEventRecord>, DatabaseError> {
        Ok(vec![])
    }
}
