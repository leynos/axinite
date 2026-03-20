//! Claude Code bridge for sandboxed execution.
//!
//! Spawns the `claude` CLI inside a Docker container and streams its NDJSON
//! output back to the orchestrator via HTTP. Supports follow-up prompts via
//! `--resume`.

mod fs_setup;
mod ndjson;
mod orchestration;
mod reporting;
mod session;
#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use crate::error::WorkerError;
use crate::worker::api::WorkerHttpClient;

/// Configuration for the Claude bridge runtime.
pub struct ClaudeBridgeConfig {
    pub job_id: Uuid,
    pub orchestrator_url: String,
    pub max_turns: u32,
    pub model: String,
    pub timeout: Duration,
    /// Tool patterns to auto-approve via project-level settings.json.
    pub allowed_tools: Vec<String>,
}

/// The Claude Code bridge runtime.
pub struct ClaudeBridgeRuntime {
    config: ClaudeBridgeConfig,
    client: Arc<WorkerHttpClient>,
}

impl ClaudeBridgeRuntime {
    /// Create a new bridge runtime.
    ///
    /// Reads `IRONCLAW_WORKER_TOKEN` from the environment for auth.
    pub fn new(config: ClaudeBridgeConfig) -> Result<Self, WorkerError> {
        let client = Arc::new(WorkerHttpClient::from_env(
            config.orchestrator_url.clone(),
            config.job_id,
        )?);

        Ok(Self { config, client })
    }
}
