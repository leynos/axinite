//! Agent job persistence traits.
//!
//! Defines the dyn-safe [`JobStore`] and its native-async sibling
//! [`NativeJobStore`] for agent jobs, LLM calls, and estimation snapshots.

use core::future::Future;

use uuid::Uuid;

use crate::context::{ActionRecord, JobContext, JobState};
use crate::db::params::{DbFuture, EstimationActualsParams, EstimationSnapshotParams};
use crate::error::DatabaseError;
use crate::history::{AgentJobRecord, AgentJobSummary, LlmCallRecord};

/// Object-safe persistence surface for agent jobs, LLM calls, and estimation
/// snapshots.
///
/// This trait provides the dyn-safe boundary for job storage operations,
/// enabling trait-object usage (e.g., `Arc<dyn JobStore>`).  It uses boxed
/// futures ([`DbFuture`]) to maintain object safety.
///
/// Companion trait: [`NativeJobStore`] provides the same API using native
/// async traits (RPITIT).  A blanket adapter automatically bridges
/// implementations of `NativeJobStore` to satisfy this trait.
///
/// Thread-safety: All implementations must be `Send + Sync` to support
/// concurrent access.
pub trait JobStore: Send + Sync {
    fn save_job<'a>(&'a self, ctx: &'a JobContext) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_job<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<Option<JobContext>, DatabaseError>>;
    fn update_job_status<'a>(
        &'a self,
        id: Uuid,
        status: JobState,
        failure_reason: Option<&'a str>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn mark_job_stuck<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_stuck_jobs<'a>(&'a self) -> DbFuture<'a, Result<Vec<Uuid>, DatabaseError>>;
    fn list_agent_jobs<'a>(&'a self) -> DbFuture<'a, Result<Vec<AgentJobRecord>, DatabaseError>>;
    fn agent_job_summary<'a>(&'a self) -> DbFuture<'a, Result<AgentJobSummary, DatabaseError>>;
    /// Get the failure reason for a single agent job (O(1) lookup).
    fn get_agent_job_failure_reason<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<String>, DatabaseError>>;
    fn save_action<'a>(
        &'a self,
        job_id: Uuid,
        action: &'a ActionRecord,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    fn get_job_actions<'a>(
        &'a self,
        job_id: Uuid,
    ) -> DbFuture<'a, Result<Vec<ActionRecord>, DatabaseError>>;
    fn record_llm_call<'a>(
        &'a self,
        record: &'a LlmCallRecord<'a>,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn save_estimation_snapshot<'a>(
        &'a self,
        params: EstimationSnapshotParams<'a>,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    fn update_estimation_actuals<'a>(
        &'a self,
        params: EstimationActualsParams,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
}

/// Native async sibling trait for concrete job-store implementations.
pub trait NativeJobStore: Send + Sync {
    fn save_job<'a>(
        &'a self,
        ctx: &'a JobContext,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_job<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<JobContext>, DatabaseError>> + Send + 'a;
    fn update_job_status<'a>(
        &'a self,
        id: Uuid,
        status: JobState,
        failure_reason: Option<&'a str>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn mark_job_stuck<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_stuck_jobs<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<Uuid>, DatabaseError>> + Send + 'a;
    fn list_agent_jobs<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<AgentJobRecord>, DatabaseError>> + Send + 'a;
    fn agent_job_summary<'a>(
        &'a self,
    ) -> impl Future<Output = Result<AgentJobSummary, DatabaseError>> + Send + 'a;
    fn get_agent_job_failure_reason<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<String>, DatabaseError>> + Send + 'a;
    fn save_action<'a>(
        &'a self,
        job_id: Uuid,
        action: &'a ActionRecord,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    fn get_job_actions<'a>(
        &'a self,
        job_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ActionRecord>, DatabaseError>> + Send + 'a;
    fn record_llm_call<'a>(
        &'a self,
        record: &'a LlmCallRecord<'a>,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn save_estimation_snapshot<'a>(
        &'a self,
        params: EstimationSnapshotParams<'a>,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    fn update_estimation_actuals<'a>(
        &'a self,
        params: EstimationActualsParams,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
}
