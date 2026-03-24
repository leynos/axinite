//! JobStore implementation for PostgreSQL backend.

use uuid::Uuid;

use crate::context::{ActionRecord, JobContext, JobState};
use crate::db::{EstimationActualsParams, EstimationSnapshotParams, NativeJobStore};
use crate::error::DatabaseError;
use crate::history::{AgentJobRecord, AgentJobSummary, LlmCallRecord};

use super::PgBackend;

impl NativeJobStore for PgBackend {
    async fn save_job(&self, ctx: &JobContext) -> Result<(), DatabaseError> {
        self.store.save_job(ctx).await
    }

    async fn get_job(&self, id: Uuid) -> Result<Option<JobContext>, DatabaseError> {
        self.store.get_job(id).await
    }

    async fn update_job_status(
        &self,
        id: Uuid,
        status: JobState,
        failure_reason: Option<&str>,
    ) -> Result<(), DatabaseError> {
        self.store
            .update_job_status(id, status, failure_reason)
            .await
    }

    async fn mark_job_stuck(&self, id: Uuid) -> Result<(), DatabaseError> {
        self.store.mark_job_stuck(id).await
    }

    async fn get_stuck_jobs(&self) -> Result<Vec<Uuid>, DatabaseError> {
        self.store.get_stuck_jobs().await
    }

    async fn list_agent_jobs(&self) -> Result<Vec<AgentJobRecord>, DatabaseError> {
        self.store.list_agent_jobs().await
    }

    async fn agent_job_summary(&self) -> Result<AgentJobSummary, DatabaseError> {
        self.store.agent_job_summary().await
    }

    async fn get_agent_job_failure_reason(
        &self,
        id: Uuid,
    ) -> Result<Option<String>, DatabaseError> {
        self.store.get_agent_job_failure_reason(id).await
    }

    async fn save_action(&self, job_id: Uuid, action: &ActionRecord) -> Result<(), DatabaseError> {
        self.store.save_action(job_id, action).await
    }

    async fn get_job_actions(&self, job_id: Uuid) -> Result<Vec<ActionRecord>, DatabaseError> {
        self.store.get_job_actions(job_id).await
    }

    async fn record_llm_call(&self, record: &LlmCallRecord<'_>) -> Result<Uuid, DatabaseError> {
        self.store.record_llm_call(record).await
    }

    async fn save_estimation_snapshot(
        &self,
        params: EstimationSnapshotParams<'_>,
    ) -> Result<Uuid, DatabaseError> {
        let EstimationSnapshotParams {
            agent_job_id,
            estimate,
            uncertainty_note,
        } = params;
        self.store
            .save_estimation_snapshot(agent_job_id, estimate, uncertainty_note)
            .await
    }

    async fn update_estimation_actuals(
        &self,
        params: EstimationActualsParams,
    ) -> Result<(), DatabaseError> {
        let EstimationActualsParams {
            estimation_id,
            tokens_used,
        } = params;
        self.store
            .update_estimation_actuals(estimation_id, tokens_used)
            .await
    }
}
