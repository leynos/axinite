//! Restart tool for graceful process restart.
//!
//! ## Architecture
//!
//! Axinite runs inside a Docker container with an entrypoint loop that monitors exit codes:
//! - **Exit code 0** (clean): Reset failure counter, wait `AXINITE_RESTART_DELAY` (default 5s), restart
//! - **Exit code ≠ 0** (failure): Increment failure counter, exit after `AXINITE_MAX_FAILURES` (default 10)
//!
//! This tool triggers a restart by calling `std::process::exit(0)` after a brief delay, allowing
//! the HTTP response to be flushed before the process terminates. The entrypoint loop then
//! detects the clean exit and automatically restarts the process.
//!
//! ## Security
//!
//! - **Approval Model:** User approval happens at the command level via web modal confirmation,
//!   not at tool execution level. This allows approved commands to execute in autonomous jobs.
//! - **Web-Only Access:** The `/restart` command only works via the web gateway (enforced in commands.rs)
//! - **Parameter Validation:** Delay clamped to 1-30 seconds
//!
//! ## Known Limitations
//!
//! - Hard exit without graceful shutdown (no destructor cleanup, no RwLock drains)
//! - In-flight jobs are paused during restart and resumed by the entrypoint
//! - Future: Implement graceful shutdown with CancellationToken for proper resource cleanup

use std::time::Duration;

use crate::context::JobContext;
#[allow(unused_imports)]
use crate::tools::tool::{ApprovalRequirement, NativeTool, ToolError, ToolOutput};

/// Tool for triggering a graceful process restart via exit code 0.
///
/// This tool signals the Docker entrypoint loop to restart the process by exiting cleanly
/// (exit code 0). User approval happens at the command level (via the web modal confirmation),
/// not at tool execution level. The `/restart` command is only callable via the web gateway
/// interface to prevent unauthorized restarts.
pub struct RestartTool;

impl NativeTool for RestartTool {
    fn name(&self) -> &str {
        "restart"
    }

    fn description(&self) -> &str {
        "Restart the Axinite agent process. The process exits cleanly (code 0) and the \
         container entrypoint loop restarts it automatically within a few seconds."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "delay_secs": {
                    "type": "integer",
                    "description": "Seconds to wait before exiting (default: 2, min: 1, max: 30)",
                    "minimum": 1,
                    "maximum": 30
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        tracing::info!("[RestartTool::execute] Restart tool invoked");
        let start = std::time::Instant::now();

        // Check if running inside a Docker container via AXINITE_IN_DOCKER env var.
        // The Docker entrypoint sets this to "true". For local development, it's unset or "false".
        // The entrypoint restart loop only works inside a Docker container (axinite-worker).
        let in_docker = std::env::var("AXINITE_IN_DOCKER")
            .map(|v| v.to_lowercase() == "true")
            .unwrap_or(false);

        tracing::debug!("[RestartTool::execute] AXINITE_IN_DOCKER={}", in_docker);

        if !in_docker {
            tracing::error!("[RestartTool::execute] Not in Docker, rejecting restart");
            return Err(ToolError::ExecutionFailed(
                "Restart is only available when running inside the Docker container. \
                 For local development, please restart Axinite manually."
                    .to_string(),
            ));
        }

        // Extract delay_secs parameter, defaulting to 2 seconds
        let delay = params
            .get("delay_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(2)
            // Validate delay against schema bounds (1-30 seconds)
            .clamp(1, 30);
        tracing::info!("[RestartTool::execute] Delay set to {} seconds", delay);

        // Spawn a background task so the response is flushed before exit.
        // We use std::process::exit(0) to trigger a Docker container restart:
        //
        // - The axinite-worker Docker container runs an entrypoint loop that monitors
        //   the exit code of the `axinite run` process:
        //   * Exit code 0 = clean restart: reset failure counter, wait AXINITE_RESTART_DELAY
        //     (default 5s), then restart the process
        //   * Exit code ≠ 0 = failure: increment counter, exit after AXINITE_MAX_FAILURES
        //     (default 10 failures)
        //
        // - std::process::exit(0) is a hard exit (no destructors, no graceful shutdown).
        //   This is intentional because:
        //   1. The HTTP response must be sent before exit (hence tokio::spawn + delay)
        //   2. In-flight jobs are paused/resumed by the entrypoint loop
        //   3. Database connections are pooled and reopened on restart
        //   4. The brief delay allows the response to flush before termination
        //
        // - Future improvement: implement graceful shutdown with CancellationToken
        //   to properly drain Axum, close DB connections, and checkpoint jobs.
        // Check if restart is disabled (e.g., in tests). This allows tests to verify
        // parameter parsing and output without actually terminating the process.
        let restart_disabled = std::env::var("AXINITE_DISABLE_RESTART")
            .map(|v| {
                let v = v.to_lowercase();
                v == "1" || v == "true"
            })
            .unwrap_or(false);

        tracing::info!(
            "[RestartTool::execute] Spawning background task to exit in {} seconds (disabled={})",
            delay,
            restart_disabled
        );
        tokio::spawn(async move {
            tracing::info!("[RestartTool] Sleeping for {} seconds before exit", delay);
            tokio::time::sleep(Duration::from_secs(delay)).await;
            if !restart_disabled {
                tracing::warn!("[RestartTool] Calling std::process::exit(0) NOW");
                std::process::exit(0);
            } else {
                tracing::info!(
                    "[RestartTool] Exit disabled (AXINITE_DISABLE_RESTART set), skipping std::process::exit(0)"
                );
            }
        });

        let msg = format!(
            "Restarting in {delay} second(s). The process will exit cleanly and the \
             entrypoint restart loop will bring Axinite back online."
        );
        tracing::info!("[RestartTool::execute] Returning success response: {}", msg);
        Ok(ToolOutput::text(msg, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }

    // NOTE: Approval is handled at the command level (/restart via web modal confirmation),
    // not at the tool execution level. By the time the tool executes, the user has already
    // confirmed via the web interface. So we don't require approval here.
    // This allows the tool to execute in autonomous jobs created from approved commands.
}

#[cfg(test)]
mod tests;
