//! LLM-facing tools for managing routines.
//!
//! Seven tools let the agent manage routines conversationally:
//! - `routine_create` - Create a new routine
//! - `routine_list` - List all routines with status
//! - `routine_update` - Modify or toggle a routine
//! - `routine_delete` - Remove a routine
//! - `routine_fire` - Manually trigger a routine
//! - `routine_history` - View past runs
//! - `event_emit` - Emit a structured system event to `system_event`-triggered routines

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use crate::agent::routine::{
    NotifyConfig, Routine, RoutineAction, RoutineGuardrails, Trigger, next_cron_fire,
};
use crate::agent::routine_engine::RoutineEngine;
use crate::context::JobContext;
use crate::db::Database;
use crate::tools::tool::HostedToolEligibility;
use crate::tools::tool::{ApprovalRequirement, Tool, ToolError, ToolOutput, require_str};

mod create;
mod delete;
mod emit;
mod fire;
mod history;
mod list;
mod update;

pub use create::RoutineCreateTool;
pub use delete::RoutineDeleteTool;
pub use emit::EventEmitTool;
pub use fire::RoutineFireTool;
pub use history::RoutineHistoryTool;
pub use list::RoutineListTool;
pub use update::RoutineUpdateTool;
