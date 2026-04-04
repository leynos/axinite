//! Agent job persistence traits.
//!
//! Defines the dyn-safe [`JobStore`] and its native-async sibling
//! [`NativeJobStore`] for agent jobs, LLM calls, and estimation snapshots.

use core::future::Future;

use rust_decimal::Decimal;
use uuid::Uuid;

use crate::context::{ActionRecord, JobContext, JobState};
use crate::db::params::DbFuture;
use crate::error::DatabaseError;
use crate::history::{AgentJobRecord, AgentJobSummary, LlmCallRecord};

/// Parameters for `save_estimation_snapshot`.
pub struct EstimationSnapshotParams<'a> {
    /// Job UUID that produced the estimation snapshot.
    pub job_id: Uuid,
    /// Job category used for the snapshot (for example "agent" or "routine").
    pub category: &'a str,
    /// Ordered tool names included in the estimation input.
    pub tool_names: &'a [String],
    /// Estimated monetary cost for the planned work.
    pub estimated_cost: Decimal,
    /// Estimated runtime in seconds.
    pub estimated_time_secs: i32,
    /// Estimated business value for the work.
    pub estimated_value: Decimal,
}

/// Parameters for `update_estimation_actuals`.
pub struct EstimationActualsParams {
    /// Snapshot UUID to update with actual values.
    pub id: Uuid,
    /// Actual observed monetary cost.
    pub actual_cost: Decimal,
    /// Actual observed runtime in seconds.
    pub actual_time_secs: i32,
    /// Actual observed business value, when available.
    pub actual_value: Option<Decimal>,
}

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
    /// Persist the supplied job context.
    ///
    /// Implementations may insert or update the row and return
    /// `DatabaseError` on any storage failure.
    fn save_job<'a>(&'a self, ctx: &'a JobContext) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Load one job context by ID.
    ///
    /// Returns `Ok(None)` when the job does not exist.
    fn get_job<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<Option<JobContext>, DatabaseError>>;
    /// Persist a status transition and optional failure reason for a job.
    ///
    /// Callers must pass the desired durable state; persistence issues surface
    /// as `DatabaseError`.
    fn update_job_status<'a>(
        &'a self,
        id: Uuid,
        status: JobState,
        failure_reason: Option<&'a str>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Mark a job as stuck in durable storage.
    fn mark_job_stuck<'a>(&'a self, id: Uuid) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Return the IDs of jobs currently persisted as stuck.
    fn get_stuck_jobs<'a>(&'a self) -> DbFuture<'a, Result<Vec<Uuid>, DatabaseError>>;
    /// List persisted non-sandbox agent jobs.
    ///
    /// Ordering is backend-defined but is typically newest first.
    fn list_agent_jobs<'a>(&'a self) -> DbFuture<'a, Result<Vec<AgentJobRecord>, DatabaseError>>;
    /// Summarise agent-job counts grouped by persisted status buckets.
    fn agent_job_summary<'a>(&'a self) -> DbFuture<'a, Result<AgentJobSummary, DatabaseError>>;
    /// Get the failure reason for a single agent job (O(1) lookup).
    fn get_agent_job_failure_reason<'a>(
        &'a self,
        id: Uuid,
    ) -> DbFuture<'a, Result<Option<String>, DatabaseError>>;
    /// Persist one action record for a job.
    ///
    /// Callers retain ownership of `action`; the store copies the relevant
    /// fields into durable storage.
    fn save_action<'a>(
        &'a self,
        job_id: Uuid,
        action: &'a ActionRecord,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
    /// Load all persisted actions for a job.
    fn get_job_actions<'a>(
        &'a self,
        job_id: Uuid,
    ) -> DbFuture<'a, Result<Vec<ActionRecord>, DatabaseError>>;
    /// Persist accounting data for one LLM call and return its row ID.
    fn record_llm_call<'a>(
        &'a self,
        record: &'a LlmCallRecord<'a>,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    /// Persist an estimation snapshot and return its snapshot ID.
    fn save_estimation_snapshot<'a>(
        &'a self,
        params: EstimationSnapshotParams<'a>,
    ) -> DbFuture<'a, Result<Uuid, DatabaseError>>;
    /// Update actual values for an existing estimation snapshot.
    ///
    /// Returns `DatabaseError` when the snapshot is missing or the update
    /// fails.
    fn update_estimation_actuals<'a>(
        &'a self,
        params: EstimationActualsParams,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;
}

/// Native async sibling trait for concrete job-store implementations.
pub trait NativeJobStore: Send + Sync {
    /// Persist the supplied job context.
    fn save_job<'a>(
        &'a self,
        ctx: &'a JobContext,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Load one job context by ID, returning `Ok(None)` when absent.
    fn get_job<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<JobContext>, DatabaseError>> + Send + 'a;
    /// Persist a status transition and optional failure reason for a job.
    fn update_job_status<'a>(
        &'a self,
        id: Uuid,
        status: JobState,
        failure_reason: Option<&'a str>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Mark a job as stuck in durable storage.
    fn mark_job_stuck<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Return the IDs of jobs currently persisted as stuck.
    fn get_stuck_jobs<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<Uuid>, DatabaseError>> + Send + 'a;
    /// List persisted non-sandbox agent jobs.
    fn list_agent_jobs<'a>(
        &'a self,
    ) -> impl Future<Output = Result<Vec<AgentJobRecord>, DatabaseError>> + Send + 'a;
    /// Summarise agent-job counts grouped by persisted status buckets.
    fn agent_job_summary<'a>(
        &'a self,
    ) -> impl Future<Output = Result<AgentJobSummary, DatabaseError>> + Send + 'a;
    /// Get the persisted failure reason for one job.
    fn get_agent_job_failure_reason<'a>(
        &'a self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<String>, DatabaseError>> + Send + 'a;
    /// Persist one action record for a job.
    fn save_action<'a>(
        &'a self,
        job_id: Uuid,
        action: &'a ActionRecord,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
    /// Load all persisted actions for a job.
    fn get_job_actions<'a>(
        &'a self,
        job_id: Uuid,
    ) -> impl Future<Output = Result<Vec<ActionRecord>, DatabaseError>> + Send + 'a;
    /// Persist accounting data for one LLM call and return its row ID.
    fn record_llm_call<'a>(
        &'a self,
        record: &'a LlmCallRecord<'a>,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    /// Persist an estimation snapshot and return its snapshot ID.
    fn save_estimation_snapshot<'a>(
        &'a self,
        params: EstimationSnapshotParams<'a>,
    ) -> impl Future<Output = Result<Uuid, DatabaseError>> + Send + 'a;
    /// Update actual values for an existing estimation snapshot.
    fn update_estimation_actuals<'a>(
        &'a self,
        params: EstimationActualsParams,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
}
