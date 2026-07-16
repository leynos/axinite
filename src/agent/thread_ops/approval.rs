//! Approval and auth-intercept flows for thread operations.
//!
//! This module manages the approval and authentication state machines for tool execution.
//!
//! ## State Machine
//!
//! The approval flow follows this state progression:
//! - **Initial/Unapproved**: Tool execution requires user approval
//! - **Pending Approval**: Thread enters `AwaitingApproval` state with `PendingApproval` stored
//! - **Approved/Authorised**: User approves; tool executes and thread returns to `Idle`
//! - **Rejected/Terminated**: User rejects; thread returns to `Idle` with rejection recorded
//!
//! The auth flow follows this progression:
//! - **Auth Required**: Extension requires authentication token
//! - **Pending Auth**: Thread has `pending_auth` set; next user message is intercepted
//! - **Authenticated**: Token provided and validated; extension activated
//! - **Auth Failed**: Token invalid; re-enters auth mode for retry
//!
//! ## Entry Points
//!
//! - `process_approval`: Called by the dispatch layer when user approves/rejects a pending tool.
//!   Caller must ensure thread is in `AwaitingApproval` state with valid `PendingApproval`.
//!
//! - `process_auth_token`: Called when user provides auth token while thread has `pending_auth`.
//!   Caller must ensure thread has valid `PendingAuth` and handle retry on failure.
//!
//! ## Invariants
//!
//! - Callers must hold valid thread metadata (thread_id, session) before invoking.
//! - Idempotent retries are supported; duplicate approvals with same request_id are ignored.
//! - State transitions are atomic under the session lock.
//! - Side effects (DB persistence, status updates) occur after state transitions complete.
//! - Concurrency: Single-writer assumption per thread; session lock must be held for state changes.
//!
//! The module is organized into submodules by responsibility:
//! - `context`: Message environment, turn scope, and approval parameter types
//! - `primary`: The `process_approval` entry point and primary tool execution
//! - `deferred_preflight`: Hook checks and approval gating for deferred tool calls
//! - `deferred_exec`: Inline and parallel execution of runnable deferred tools
//! - `deferred_flow`: Orchestration of the deferred-tools continuation
//! - `turn_flow`: Turn finalization, rejection, and loop continuation
//! - `auth`: Auth intercepts and the `process_auth_token` entry point

mod auth;
mod context;
mod deferred_exec;
mod deferred_flow;
mod deferred_preflight;
mod primary;
mod turn_flow;

pub(crate) use context::{ApprovalParams, TurnScope};

#[cfg(test)]
#[path = "approval_tests.rs"]
mod tests;
