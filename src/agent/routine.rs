//! Core types for the routines system.
//!
//! A routine is a named, persistent, user-owned task with a trigger and an action.
//! Each routine fires independently when its trigger condition is met, with only
//! that routine's prompt and context sent to the LLM.
//!
//! ```text
//! ┌──────────┐     ┌─────────┐     ┌──────────────────┐
//! │  Trigger  │────▶│ Engine  │────▶│  Execution Mode  │
//! │ cron/event│     │guardrail│     │lightweight│full_job│
//! │ system    │     │ check   │     └──────────────────┘
//! │ manual    │     └─────────┘              │
//! └──────────┘                               ▼
//!                                     ┌──────────────┐
//!                                     │  Notify user │
//!                                     │  if needed   │
//!                                     └──────────────┘
//! ```

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::RoutineError;

mod action;
mod trigger;

pub use action::{RoutineAction, parse_tool_permissions};
pub use trigger::{Trigger, json_value_as_filter_string, next_cron_fire};

/// A routine is a named, persistent, user-owned task with a trigger and an action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Routine {
    /// Unique identifier of the routine.
    pub id: Uuid,
    /// Name, unique per owning user.
    pub name: String,
    /// Free-text summary of what the routine is for.
    pub description: String,
    /// Owning user. Event triggers only match messages from this user.
    pub user_id: String,
    /// When `false`, triggers skip the routine and manual fires are refused.
    pub enabled: bool,
    /// When the routine fires: a cron schedule, an event, a system event or manual.
    pub trigger: Trigger,
    /// What each run does: a lightweight LLM call or a full job.
    pub action: RoutineAction,
    /// Limits on how often the routine runs and how many runs may overlap.
    pub guardrails: RoutineGuardrails,
    /// Where and when run results are announced.
    pub notify: NotifyConfig,

    // Runtime state (DB-managed)
    /// When the last run's runtime state was recorded. Cooldown is measured from this time.
    pub last_run_at: Option<DateTime<Utc>>,
    /// Next scheduled cron fire. `None` for non-cron triggers or when no further fire is due.
    pub next_fire_at: Option<DateTime<Utc>>,
    /// Total number of runs recorded for this routine.
    pub run_count: u64,
    /// Consecutive runs that ended `Failed`. Any other status resets it to zero.
    pub consecutive_failures: u32,
    /// Opaque JSON persisted with the routine. The engine stores it unchanged and does not read it.
    pub state: serde_json::Value,

    /// When the routine was created.
    pub created_at: DateTime<Utc>,
    /// When the routine row was last written, including runtime-state updates.
    pub updated_at: DateTime<Utc>,
}

/// Extract a required string field from a JSON config blob.
///
/// Returns `RoutineError::MissingField` naming the surrounding `context`
/// when the field is absent or not a string.
fn required_str_field(
    config: &serde_json::Value,
    context: &str,
    field: &str,
) -> Result<String, RoutineError> {
    config
        .get(field)
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| RoutineError::MissingField {
            context: context.into(),
            field: field.into(),
        })
}

/// Guardrails to prevent runaway execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineGuardrails {
    /// Minimum time between fires.
    pub cooldown: Duration,
    /// Max simultaneous runs of this routine.
    pub max_concurrent: u32,
    /// Window for content-hash dedup (event triggers). None = no dedup.
    pub dedup_window: Option<Duration>,
}

impl Default for RoutineGuardrails {
    fn default() -> Self {
        Self {
            cooldown: Duration::from_secs(300),
            max_concurrent: 1,
            dedup_window: None,
        }
    }
}

/// Notification preferences for a routine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyConfig {
    /// Channel to notify on (None = default/broadcast all).
    pub channel: Option<String>,
    /// User to notify.
    pub user: String,
    /// Notify when routine produces actionable output.
    pub on_attention: bool,
    /// Notify when routine errors.
    pub on_failure: bool,
    /// Notify when routine runs with no findings.
    pub on_success: bool,
}

impl Default for NotifyConfig {
    fn default() -> Self {
        Self {
            channel: None,
            user: "default".to_string(),
            on_attention: true,
            on_failure: true,
            on_success: false,
        }
    }
}

/// Status of a routine run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// The run has started and has not finished.
    Running,
    /// The run finished with nothing to report (the agent replied `ROUTINE_OK`).
    Ok,
    /// The run finished with output the user should see.
    Attention,
    /// The run ended in an error.
    Failed,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunStatus::Running => write!(f, "running"),
            RunStatus::Ok => write!(f, "ok"),
            RunStatus::Attention => write!(f, "attention"),
            RunStatus::Failed => write!(f, "failed"),
        }
    }
}

impl FromStr for RunStatus {
    type Err = RoutineError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "running" => Ok(RunStatus::Running),
            "ok" => Ok(RunStatus::Ok),
            "attention" => Ok(RunStatus::Attention),
            "failed" => Ok(RunStatus::Failed),
            other => Err(RoutineError::UnknownRunStatus {
                status: other.to_string(),
            }),
        }
    }
}

/// A single execution of a routine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineRun {
    /// Unique identifier of this run.
    pub id: Uuid,
    /// The routine this run belongs to.
    pub routine_id: Uuid,
    /// What fired the run: `manual`, `cron`, `event` or `system_event`.
    pub trigger_type: String,
    /// Trigger context. The cron schedule for `cron`, the message text for `event` (cut to
    /// 200 bytes and suffixed with `...`), `source:event_type` for `system_event`, `None`
    /// for `manual`.
    pub trigger_detail: Option<String>,
    /// When the run started.
    pub started_at: DateTime<Utc>,
    /// When the run finished. `None` while it is still running.
    pub completed_at: Option<DateTime<Utc>>,
    /// Outcome of the run.
    pub status: RunStatus,
    /// Agent reply for an `Attention` run, or the error text for a `Failed` run. `None` otherwise.
    pub result_summary: Option<String>,
    /// Input plus output tokens the run consumed. `None` when not recorded.
    pub tokens_used: Option<i32>,
    /// Job created for a `FullJob` run. `None` for lightweight runs.
    pub job_id: Option<Uuid>,
    /// When the run record was created.
    pub created_at: DateTime<Utc>,
}

/// Compute a content hash for event dedup.
pub fn content_hash(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests;
