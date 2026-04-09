//! Null implementation of NativeJobStore for NullDatabase.

use uuid::Uuid;

use crate::context::{ActionRecord, JobContext};
use crate::db::{EstimationActualsParams, EstimationSnapshotParams};
use crate::error::DatabaseError;
use crate::history::{AgentJobRecord, AgentJobSummary, LlmCallRecord};

use super::NullDatabase;

impl crate::db::NativeJobStore for NullDatabase {
    async fn save_job(&self, _ctx: &JobContext) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_job(&self, _id: Uuid) -> Result<Option<JobContext>, DatabaseError> {
        Ok(None)
    }

    async fn update_job_status(
        &self,
        _id: Uuid,
        _status: crate::context::JobState,
        _failure_reason: Option<&str>,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn mark_job_stuck(&self, _id: Uuid) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_stuck_jobs(&self) -> Result<Vec<Uuid>, DatabaseError> {
        Ok(vec![])
    }

    async fn list_agent_jobs(&self) -> Result<Vec<AgentJobRecord>, DatabaseError> {
        Ok(vec![])
    }

    async fn agent_job_summary(&self) -> Result<AgentJobSummary, DatabaseError> {
        Ok(AgentJobSummary::default())
    }

    async fn get_agent_job_failure_reason(
        &self,
        _id: Uuid,
    ) -> Result<Option<String>, DatabaseError> {
        Ok(None)
    }

    async fn save_action(
        &self,
        _job_id: Uuid,
        _action: &ActionRecord,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }

    async fn get_job_actions(&self, _job_id: Uuid) -> Result<Vec<ActionRecord>, DatabaseError> {
        Ok(vec![])
    }

    async fn record_llm_call(&self, _record: &LlmCallRecord<'_>) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn save_estimation_snapshot(
        &self,
        _params: EstimationSnapshotParams<'_>,
    ) -> Result<Uuid, DatabaseError> {
        Ok(Uuid::new_v4())
    }

    async fn update_estimation_actuals(
        &self,
        _params: EstimationActualsParams,
    ) -> Result<(), DatabaseError> {
        Ok(())
    }
}
