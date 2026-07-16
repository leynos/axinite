//! Submission and submission-result types for the turn-based agent loop.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A submission to the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Submission {
    /// User text input (starts a new turn).
    UserInput {
        /// The user's message content.
        content: String,
    },

    /// Response to an execution approval request (with explicit request ID).
    ExecApproval {
        /// ID of the approval request being responded to.
        request_id: Uuid,
        /// Whether the execution was approved.
        approved: bool,
        /// If true, auto-approve this tool for the rest of the session.
        always: bool,
    },

    /// Simple approval response (yes/no/always) for the current pending approval.
    ApprovalResponse {
        /// Whether the execution was approved.
        approved: bool,
        /// If true, auto-approve this tool for the rest of the session.
        always: bool,
    },

    /// Interrupt the current turn.
    Interrupt,

    /// Request context compaction.
    Compact,

    /// Undo the last turn.
    Undo,

    /// Redo a previously undone turn (if available).
    Redo,

    /// Resume from a specific checkpoint.
    Resume {
        /// ID of the checkpoint to resume from.
        checkpoint_id: Uuid,
    },

    /// Clear the current thread and start fresh.
    Clear,

    /// Switch to a different thread.
    SwitchThread {
        /// ID of the thread to switch to.
        thread_id: Uuid,
    },

    /// Create a new thread.
    NewThread,

    /// Trigger a manual heartbeat check.
    Heartbeat,

    /// Summarize the current thread.
    Summarize,

    /// Suggest next steps based on the current thread.
    Suggest,

    /// Check job status. No job_id shows all jobs; with job_id shows a specific job.
    JobStatus {
        /// Optional job ID (UUID or short prefix). If None, shows all jobs.
        job_id: Option<String>,
    },

    /// Cancel a running job.
    JobCancel {
        /// Job ID (UUID or short prefix).
        job_id: String,
    },

    /// Quit the agent. Bypasses thread-state checks.
    Quit,

    /// System command (help, model, version, tools, ping, debug).
    /// Bypasses thread-state checks and safety validation.
    SystemCommand {
        /// The command name (e.g. "help", "model", "version").
        command: String,
        /// Arguments to the command.
        args: Vec<String>,
    },
}

impl Submission {
    /// Create a user input submission.
    pub fn user_input(content: impl Into<String>) -> Self {
        Self::UserInput {
            content: content.into(),
        }
    }

    /// Create an approval submission.
    #[cfg(test)]
    pub fn approval(request_id: Uuid, approved: bool) -> Self {
        Self::ExecApproval {
            request_id,
            approved,
            always: false,
        }
    }

    /// Create an "always approve" submission.
    #[cfg(test)]
    pub fn always_approve(request_id: Uuid) -> Self {
        Self::ExecApproval {
            request_id,
            approved: true,
            always: true,
        }
    }

    /// Create an interrupt submission.
    #[cfg(test)]
    pub fn interrupt() -> Self {
        Self::Interrupt
    }

    /// Create a compact submission.
    #[cfg(test)]
    pub fn compact() -> Self {
        Self::Compact
    }

    /// Create an undo submission.
    #[cfg(test)]
    pub fn undo() -> Self {
        Self::Undo
    }

    /// Create a redo submission.
    #[cfg(test)]
    pub fn redo() -> Self {
        Self::Redo
    }

    /// Check if this submission starts a new turn.
    #[cfg(test)]
    pub fn starts_turn(&self) -> bool {
        matches!(self, Self::UserInput { .. })
    }

    /// Check if this submission is a control command.
    pub fn is_control(&self) -> bool {
        matches!(
            self,
            Self::Interrupt
                | Self::Compact
                | Self::Undo
                | Self::Redo
                | Self::Clear
                | Self::NewThread
                | Self::Heartbeat
                | Self::Summarize
                | Self::Suggest
                | Self::JobStatus { .. }
                | Self::JobCancel { .. }
                | Self::SystemCommand { .. }
        )
    }
}

/// Result of processing a submission.
#[derive(Debug, Clone)]
pub enum SubmissionResult {
    /// Turn completed with a response.
    Response {
        /// The agent's response.
        content: String,
    },

    /// Need approval before continuing.
    NeedApproval {
        /// ID of the approval request.
        request_id: Uuid,
        /// Tool that needs approval.
        tool_name: String,
        /// Description of what the tool will do.
        description: String,
        /// Parameters being passed.
        parameters: serde_json::Value,
    },

    /// Successfully processed (for control commands).
    Ok {
        /// Optional message.
        message: Option<String>,
    },

    /// Error occurred.
    Error {
        /// Error message.
        message: String,
    },

    /// Turn was interrupted.
    Interrupted,
}

impl SubmissionResult {
    /// Create a response result.
    pub fn response(content: impl Into<String>) -> Self {
        Self::Response {
            content: content.into(),
        }
    }

    /// Create an OK result.
    #[cfg(test)]
    pub fn ok() -> Self {
        Self::Ok { message: None }
    }

    /// Create an OK result with a message.
    pub fn ok_with_message(message: impl Into<String>) -> Self {
        Self::Ok {
            message: Some(message.into()),
        }
    }

    /// Create an error result.
    pub fn error(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
        }
    }
}
