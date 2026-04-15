//! Top-level database supertrait.
//!
//! Combines all sub-traits into a single [`Database`] surface that existing
//! `Arc<dyn Database>` consumers continue to use.

use core::future::Future;

use uuid::Uuid;

use crate::context::JobState;
use crate::db::SandboxEventType;
use crate::db::params::DbFuture;
use crate::error::DatabaseError;

use super::conversation::{ConversationStore, NativeConversationStore};
use super::job::{JobStore, NativeJobStore};
use super::routine::{NativeRoutineStore, RoutineStore};
use super::sandbox::{NativeSandboxStore, SandboxStore};
use super::settings::{NativeSettingsStore, SettingsStore};
use super::tool_failure::{NativeToolFailureStore, ToolFailureStore};
use super::workspace::{NativeWorkspaceStore, WorkspaceStore};

/// Backend-agnostic database supertrait.
///
/// Combines all sub-traits into one.  Existing `Arc<dyn Database>` consumers
/// continue to work; leaf consumers can depend on a specific sub-trait instead.
pub trait Database:
    ConversationStore
    + JobStore
    + SandboxStore
    + RoutineStore
    + ToolFailureStore
    + SettingsStore
    + WorkspaceStore
    + Send
    + Sync
{
    /// Parameters for atomically persisting a terminal job event and status.
    fn persist_terminal_result_and_status<'a>(
        &'a self,
        params: TerminalJobPersistence<'a>,
    ) -> DbFuture<'a, Result<(), DatabaseError>>;

    /// Apply all pending schema migrations before the backend is used.
    ///
    /// Implementations must be idempotent, so callers may safely invoke this
    /// more than once during startup without reapplying completed work. The
    /// method is async and non-blocking from the caller's perspective, and
    /// returns `Ok(())` once the schema is ready for use.
    ///
    /// Returns `Err(DatabaseError)` when migration fails. Such failures are
    /// fatal for the backend instance, which should not be used afterwards.
    /// Typical call sites run migrations immediately after constructing the
    /// backend and before exposing it to the rest of the application.
    fn run_migrations<'a>(&'a self) -> DbFuture<'a, Result<(), DatabaseError>>;
}

/// Native async sibling trait for concrete database implementations.
pub trait NativeDatabase:
    NativeConversationStore
    + NativeJobStore
    + NativeSandboxStore
    + NativeRoutineStore
    + NativeToolFailureStore
    + NativeSettingsStore
    + NativeWorkspaceStore
    + Send
    + Sync
{
    /// Native async form of [`Database::persist_terminal_result_and_status`].
    fn persist_terminal_result_and_status<'a>(
        &'a self,
        params: TerminalJobPersistence<'a>,
    ) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;

    /// Apply all pending schema migrations before the backend is used.
    ///
    /// Implementations must be idempotent, so callers may safely invoke this
    /// more than once during startup without reapplying completed work. The
    /// returned future must stay `Send` and borrow `self` for the lifetime
    /// `'a`, allowing concrete backends to perform async migration work
    /// without blocking the calling thread.
    ///
    /// Returns `Ok(())` once the schema is ready for use, or
    /// `Err(DatabaseError)` when migration fails. Migration errors are fatal
    /// for the backend instance, which should not be used afterwards. Typical
    /// call sites run this once immediately after backend construction.
    fn run_migrations<'a>(&'a self) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
}

/// Parameters for atomically persisting a terminal event and terminal status.
pub struct TerminalJobPersistence<'a> {
    /// Direct agent job UUID being updated.
    pub job_id: Uuid,
    /// Terminal job status to persist.
    pub status: JobState,
    /// Optional failure or completion reason to persist on the job row.
    pub failure_reason: Option<&'a str>,
    /// Event type written to `job_events`.
    pub event_type: SandboxEventType,
    /// Structured event payload written alongside the status transition.
    pub event_data: &'a serde_json::Value,
}
