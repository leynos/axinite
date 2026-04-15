//! Trait definitions for each database store family.
//!
//! Each submodule owns one dyn-safe / native-async trait pair.  The parent
//! `db` module re-exports these traits so that external code continues to
//! use `crate::db::{ConversationStore, NativeConversationStore, …}`.

pub mod conversation;
pub mod database;
pub mod job;
pub mod routine;
pub mod sandbox;
pub mod settings;
pub mod tool_failure;
pub mod workspace;

pub use conversation::{ConversationStore, NativeConversationStore};
pub use database::{Database, NativeDatabase, TerminalJobPersistence};
pub use job::{JobStore, NativeJobStore};
pub use routine::{NativeRoutineStore, RoutineStore};
pub use sandbox::{NativeSandboxStore, SandboxStore};
pub use settings::{NativeSettingsStore, SettingsStore};
pub use tool_failure::{NativeToolFailureStore, ToolFailureStore};
pub use workspace::{NativeWorkspaceStore, WorkspaceStore};
