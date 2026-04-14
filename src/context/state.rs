//! Job state machine.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::llm::recording::HttpInterceptor;

/// Errors that can occur during job recovery.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum JobRecoveryError {
    /// Job is not in the Stuck state and cannot be recovered.
    #[error("Job is not stuck")]
    NotStuck,
}

/// State of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    /// Job is waiting to be started.
    Pending,
    /// Job is currently being worked on.
    InProgress,
    /// Job work is complete, awaiting submission.
    Completed,
    /// Job has been submitted for review.
    Submitted,
    /// Job was accepted/paid.
    Accepted,
    /// Job failed and cannot be completed.
    Failed,
    /// Job is stuck and needs repair.
    Stuck,
    /// Job was cancelled.
    Cancelled,
}

impl JobState {
    /// Check if this state allows transitioning to another state.
    pub fn can_transition_to(&self, target: JobState) -> bool {
        use JobState::*;

        matches!(
            (self, target),
            // From Pending
            (Pending, InProgress) | (Pending, Cancelled) |
            // From InProgress
            (InProgress, Completed) | (InProgress, Failed) |
            (InProgress, Stuck) | (InProgress, Cancelled) |
            // From Completed
            (Completed, Submitted) | (Completed, Failed) |
            // From Submitted
            (Submitted, Accepted) | (Submitted, Failed) |
            // From Stuck (can recover or fail)
            (Stuck, InProgress) | (Stuck, Failed) | (Stuck, Cancelled)
        )
    }

    /// Check if this is a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Accepted | Self::Failed | Self::Cancelled)
    }

    /// Check if the job is active (not terminal).
    pub fn is_active(&self) -> bool {
        !self.is_terminal()
    }
}

impl std::str::FromStr for JobState {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "submitted" => Ok(Self::Submitted),
            "accepted" => Ok(Self::Accepted),
            "failed" => Ok(Self::Failed),
            "stuck" => Ok(Self::Stuck),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for JobState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Submitted => "submitted",
            Self::Accepted => "accepted",
            Self::Failed => "failed",
            Self::Stuck => "stuck",
            Self::Cancelled => "cancelled",
        };
        write!(f, "{}", s)
    }
}

/// A state transition event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    /// Previous state.
    pub from: JobState,
    /// New state.
    pub to: JobState,
    /// When the transition occurred.
    pub timestamp: DateTime<Utc>,
    /// Reason for the transition.
    pub reason: Option<String>,
}

/// Context for a running job.
#[derive(Debug, Clone, Serialize)]
pub struct JobContext {
    /// Unique job ID.
    pub job_id: Uuid,
    /// Current state.
    pub state: JobState,
    /// User ID that owns this job (for workspace scoping).
    pub user_id: String,
    /// Conversation ID if linked to a conversation.
    pub conversation_id: Option<Uuid>,
    /// Job title.
    pub title: String,
    /// Job description.
    pub description: String,
    /// Job category.
    pub category: Option<String>,
    /// Budget amount (if from marketplace).
    pub budget: Option<Decimal>,
    /// Budget token (e.g., "NEAR", "USD").
    pub budget_token: Option<String>,
    /// Our bid amount.
    pub bid_amount: Option<Decimal>,
    /// Estimated cost to complete.
    pub estimated_cost: Option<Decimal>,
    /// Estimated time to complete.
    pub estimated_duration: Option<Duration>,
    /// Actual cost so far.
    pub actual_cost: Decimal,
    /// Total tokens consumed by LLM calls in this job.
    pub total_tokens_used: u64,
    /// Maximum tokens allowed per job (0 = unlimited).
    pub max_tokens: u64,
    /// When the job was created.
    pub created_at: DateTime<Utc>,
    /// When the job was started.
    pub started_at: Option<DateTime<Utc>>,
    /// When the job was completed.
    pub completed_at: Option<DateTime<Utc>>,
    /// Number of repair attempts.
    pub repair_attempts: u32,
    /// State transition history.
    pub transitions: Vec<StateTransition>,
    /// Metadata.
    pub metadata: serde_json::Value,
    /// Extra environment variables to inject into spawned child processes.
    ///
    /// Used by the worker runtime to pass fetched credentials to tools
    /// (e.g., shell commands) without mutating the global process environment
    /// via `std::env::set_var`, which is unsafe in multi-threaded programs.
    ///
    /// Wrapped in `Arc` for cheap cloning on every tool invocation.
    #[serde(skip)]
    pub extra_env: Arc<HashMap<String, String>>,
    /// Optional HTTP interceptor for trace recording/replay.
    ///
    /// When set, tools that make outgoing HTTP requests should check this
    /// interceptor before sending real requests. During recording, the
    /// interceptor captures request/response pairs. During replay, it
    /// returns pre-recorded responses.
    #[serde(skip)]
    pub http_interceptor: Option<Arc<dyn HttpInterceptor>>,
    /// Stash of full tool outputs keyed by tool_call_id.
    ///
    /// Tool outputs may be truncated before reaching the LLM context window,
    /// but subsequent tools (e.g., `json`) may need the full output. This
    /// stash stores the complete, unsanitized output so tools can reference
    /// previous results by ID via `$tool_call_id` parameter syntax.
    #[serde(skip)]
    pub tool_output_stash: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
    /// User's preferred timezone (IANA name, e.g. "America/New_York"). Defaults to "UTC".
    pub user_timezone: String,
}

impl JobContext {
    /// Create a new job context.
    pub fn new(title: impl Into<String>, description: impl Into<String>) -> Self {
        Self::with_user("default", title, description)
    }

    /// Create a new job context with a specific user ID.
    pub fn with_user(
        user_id: impl Into<String>,
        title: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        Self {
            job_id: Uuid::new_v4(),
            state: JobState::Pending,
            user_id: user_id.into(),
            conversation_id: None,
            title: title.into(),
            description: description.into(),
            category: None,
            budget: None,
            budget_token: None,
            bid_amount: None,
            estimated_cost: None,
            estimated_duration: None,
            actual_cost: Decimal::ZERO,
            total_tokens_used: 0,
            max_tokens: 0,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            repair_attempts: 0,
            transitions: Vec::new(),
            extra_env: Arc::new(HashMap::new()),
            http_interceptor: None,
            metadata: serde_json::Value::Null,
            tool_output_stash: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            user_timezone: "UTC".to_string(),
        }
    }

    /// Set the user timezone on this context.
    pub fn with_timezone(mut self, tz: impl Into<String>) -> Self {
        self.user_timezone = tz.into();
        self
    }

    /// Transition to a new state.
    pub fn transition_to(
        &mut self,
        new_state: JobState,
        reason: Option<String>,
    ) -> Result<(), String> {
        if !self.state.can_transition_to(new_state) {
            return Err(format!(
                "Cannot transition from {} to {}",
                self.state, new_state
            ));
        }

        let transition = StateTransition {
            from: self.state,
            to: new_state,
            timestamp: Utc::now(),
            reason,
        };

        self.transitions.push(transition);

        // Cap transition history to prevent unbounded memory growth
        const MAX_TRANSITIONS: usize = 200;
        if self.transitions.len() > MAX_TRANSITIONS {
            let drain_count = self.transitions.len() - MAX_TRANSITIONS;
            self.transitions.drain(..drain_count);
        }

        self.state = new_state;

        // Update timestamps
        match new_state {
            JobState::InProgress if self.started_at.is_none() => {
                self.started_at = Some(Utc::now());
            }
            JobState::Completed | JobState::Accepted | JobState::Failed | JobState::Cancelled => {
                self.completed_at = Some(Utc::now());
            }
            _ => {}
        }

        Ok(())
    }

    /// Check whether the newest recorded transition matches a rollback from
    /// `previous` back to the current in-memory state.
    fn last_transition_matches_rollback(&self, previous: JobState) -> bool {
        self.transitions
            .last()
            .is_some_and(|t| t.from == previous && t.to == self.state)
    }

    /// Directly set the state without transition validation.
    ///
    /// Intended for rollback paths where the in-memory context must be
    /// restored to a previous state after a persistence failure, bypassing
    /// [`Self::transition_to`] validation.
    pub(crate) fn set_state_rollback(&mut self, previous: JobState) {
        if !self.last_transition_matches_rollback(previous) {
            return;
        }

        self.transitions.pop();
        self.state = previous;
        self.completed_at = if matches!(
            self.state,
            JobState::Completed | JobState::Accepted | JobState::Failed | JobState::Cancelled
        ) {
            self.transitions
                .iter()
                .rev()
                .find(|transition| {
                    matches!(
                        transition.to,
                        JobState::Completed
                            | JobState::Accepted
                            | JobState::Failed
                            | JobState::Cancelled
                    )
                })
                .map(|transition| transition.timestamp)
        } else {
            None
        };
    }

    /// Add to the actual cost.
    pub fn add_cost(&mut self, cost: Decimal) {
        self.actual_cost += cost;
    }

    /// Record token usage from an LLM call. Returns an error string if the
    /// token budget has been exceeded after this addition.
    pub fn add_tokens(&mut self, tokens: u64) -> Result<(), String> {
        self.total_tokens_used += tokens;
        if self.max_tokens > 0 && self.total_tokens_used > self.max_tokens {
            Err(format!(
                "Token budget exceeded: used {} of {} allowed tokens",
                self.total_tokens_used, self.max_tokens
            ))
        } else {
            Ok(())
        }
    }

    /// Check whether the monetary budget has been exceeded.
    pub fn budget_exceeded(&self) -> bool {
        if let Some(ref budget) = self.budget {
            self.actual_cost > *budget
        } else {
            false
        }
    }

    /// Get the duration since the job started.
    pub fn elapsed(&self) -> Option<Duration> {
        self.started_at.map(|start| {
            let end = self.completed_at.unwrap_or_else(Utc::now);
            let duration = end.signed_duration_since(start);
            Duration::from_secs(duration.num_seconds().max(0) as u64)
        })
    }

    /// Return when the job most recently entered the stuck state.
    pub fn stuck_since(&self) -> Option<DateTime<Utc>> {
        self.transitions
            .iter()
            .rev()
            .find(|transition| transition.to == JobState::Stuck)
            .map(|transition| transition.timestamp)
    }

    /// Mark the job as stuck.
    pub fn mark_stuck(&mut self, reason: impl Into<String>) -> Result<(), String> {
        self.transition_to(JobState::Stuck, Some(reason.into()))
    }

    /// Attempt to recover from stuck state.
    pub fn attempt_recovery(&mut self) -> Result<(), JobRecoveryError> {
        if self.state != JobState::Stuck {
            return Err(JobRecoveryError::NotStuck);
        }
        self.repair_attempts += 1;
        self.transition_to(JobState::InProgress, Some("Recovery attempt".to_string()))
            .map_err(|e| panic!("Failed to transition from Stuck to InProgress: {}", e))
    }
}

impl Default for JobContext {
    fn default() -> Self {
        Self::with_user("default", "Untitled", "No description")
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
