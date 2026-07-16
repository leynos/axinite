//! System commands and job handlers for the agent.
//!
//! Extracted from `agent_loop.rs` to isolate the /help, /model, /status,
//! and other command processing from the core agent loop. Split into
//! submodules by feature area: job lifecycle, conversation helpers,
//! skills, and system commands.

mod conversation;
mod jobs;
mod skills;
mod system;

use crate::agent::Agent;
use crate::agent::submission::SubmissionResult;
use crate::error::Error;

impl Agent {
    /// Trigger a manual heartbeat check.
    pub(in crate::agent) async fn process_heartbeat(&self) -> Result<SubmissionResult, Error> {
        let Some(workspace) = self.workspace() else {
            return Ok(SubmissionResult::error(
                "Heartbeat requires a workspace (database must be connected).",
            ));
        };

        let runner = crate::agent::HeartbeatRunner::new(
            crate::agent::HeartbeatConfig::default(),
            crate::workspace::hygiene::HygieneConfig::default(),
            workspace.clone(),
            self.llm().clone(),
        );

        match runner.check_heartbeat().await {
            crate::agent::HeartbeatResult::Ok => Ok(SubmissionResult::ok_with_message(
                "Heartbeat: all clear, nothing needs attention.",
            )),
            crate::agent::HeartbeatResult::NeedsAttention(msg) => Ok(SubmissionResult::response(
                format!("Heartbeat findings:\n\n{}", msg),
            )),
            crate::agent::HeartbeatResult::Skipped => Ok(SubmissionResult::ok_with_message(
                "Heartbeat skipped: no HEARTBEAT.md checklist found in workspace.",
            )),
            crate::agent::HeartbeatResult::Failed(err) => Ok(SubmissionResult::error(format!(
                "Heartbeat failed: {}",
                err
            ))),
        }
    }
}
