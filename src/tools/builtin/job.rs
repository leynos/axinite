//! Job management tools.
//!
//! These tools allow the LLM to manage jobs:
//! - Create new jobs/tasks (with optional sandbox delegation)
//! - List existing jobs
//! - Check job status
//! - Cancel running jobs
//!
//! The module is split by concern: [`create`] holds the `create_job` tool and
//! its local execution path, [`sandbox`] the container execution path,
//! [`credentials`] credential grant parsing, [`project_dir`] project directory
//! resolution, [`status`] the list/status/cancel tools, [`interaction`]
//! the event log and follow-up prompt tools, and [`output`] shared
//! `ToolOutput` construction helpers.

use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::context::ContextManager;
use crate::tools::tool::ToolError;

mod create;
mod credentials;
mod interaction;
mod output;
mod project_dir;
mod sandbox;
mod status;

#[cfg(test)]
mod tests;

pub use create::CreateJobTool;
pub use interaction::{JobEventsTool, JobPromptTool, PromptQueue};
pub use status::{CancelJobTool, JobStatusTool, ListJobsTool};

/// Lazy scheduler reference, filled after Agent::new creates the Scheduler.
///
/// Solves the chicken-and-egg: tools are registered before the Scheduler exists
/// (Scheduler needs the ToolRegistry). Created empty, filled after Agent::new.
pub type SchedulerSlot = Arc<RwLock<Option<Arc<crate::agent::Scheduler>>>>;

/// Resolve a job ID from a full UUID or a short prefix (like git short SHAs).
///
/// Tries full UUID parse first. If that fails, treats the input as a hex prefix
/// and searches the context manager for a unique match.
async fn resolve_job_id(input: &str, context_manager: &ContextManager) -> Result<Uuid, ToolError> {
    // Fast path: full UUID
    if let Ok(id) = Uuid::parse_str(input) {
        return Ok(id);
    }

    // Require a minimum prefix length to limit brute-force enumeration.
    if input.len() < 4 {
        return Err(ToolError::InvalidParameters(
            "job ID prefix must be at least 4 hex characters".to_string(),
        ));
    }

    // Prefix match against known jobs
    let input_lower = input.to_lowercase();
    let all_ids = context_manager.all_jobs().await;
    let matches: Vec<Uuid> = all_ids
        .into_iter()
        .filter(|id| {
            let hex = id.to_string().replace('-', "");
            hex.starts_with(&input_lower)
        })
        .collect();

    match matches.len() {
        1 => Ok(matches[0]),
        0 => Err(ToolError::InvalidParameters(format!(
            "no job found matching prefix '{}'",
            input
        ))),
        n => Err(ToolError::InvalidParameters(format!(
            "ambiguous prefix '{}' matches {} jobs, provide more characters",
            input, n
        ))),
    }
}
