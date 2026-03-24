//! Top-level database supertrait.
//!
//! Combines all sub-traits into a single [`Database`] surface that existing
//! `Arc<dyn Database>` consumers continue to use.

use core::future::Future;

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
    /// Run schema migrations for this backend.
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
    fn run_migrations<'a>(&'a self) -> impl Future<Output = Result<(), DatabaseError>> + Send + 'a;
}
