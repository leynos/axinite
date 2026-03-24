//! Parameter objects and shared type aliases for the database trait surface.
//!
//! These structs reduce argument counts for database methods with many
//! parameters.  The boxed-future alias lives here so that every trait
//! submodule can import it from a single canonical location.

use core::{future::Future, pin::Pin};

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::agent::routine::RunStatus;
use crate::workspace::SearchConfig;

/// Boxed future used at dyn-backed database trait boundaries.
pub type DbFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Parameters for `ensure_conversation`.
pub struct EnsureConversationParams<'a> {
    pub id: Uuid,
    pub channel: &'a str,
    pub user_id: &'a str,
    pub thread_id: Option<&'a str>,
}

/// Parameters for `save_estimation_snapshot`.
pub struct EstimationSnapshotParams<'a> {
    pub job_id: Uuid,
    pub category: &'a str,
    pub tool_names: &'a [String],
    pub estimated_cost: Decimal,
    pub estimated_time_secs: i32,
    pub estimated_value: Decimal,
}

/// Parameters for `update_estimation_actuals`.
pub struct EstimationActualsParams {
    pub id: Uuid,
    pub actual_cost: Decimal,
    pub actual_time_secs: i32,
    pub actual_value: Option<Decimal>,
}

/// Parameters for `update_sandbox_job_status`.
pub struct SandboxJobStatusUpdate<'a> {
    pub id: Uuid,
    pub status: &'a str,
    pub success: Option<bool>,
    pub message: Option<&'a str>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

/// Parameters for `update_routine_runtime`.
pub struct RoutineRuntimeUpdate<'a> {
    pub id: Uuid,
    pub last_run_at: DateTime<Utc>,
    pub next_fire_at: Option<DateTime<Utc>>,
    pub run_count: u64,
    pub consecutive_failures: u32,
    pub state: &'a serde_json::Value,
}

/// Parameters for `complete_routine_run`.
pub struct RoutineRunCompletion<'a> {
    pub id: Uuid,
    pub status: RunStatus,
    pub result_summary: Option<&'a str>,
    pub tokens_used: Option<i32>,
}

/// Parameters for `insert_chunk`.
pub struct InsertChunkParams<'a> {
    pub document_id: Uuid,
    pub chunk_index: i32,
    pub content: &'a str,
    pub embedding: Option<&'a [f32]>,
}

/// Parameters for `hybrid_search`.
pub struct HybridSearchParams<'a> {
    pub user_id: &'a str,
    pub agent_id: Option<Uuid>,
    pub query: &'a str,
    pub embedding: Option<&'a [f32]>,
    pub config: &'a SearchConfig,
}
