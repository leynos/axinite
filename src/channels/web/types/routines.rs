//! Routine (scheduled/triggered automation) DTOs for the web gateway API.

use serde::Serialize;
use uuid::Uuid;

// --- Routines ---

/// Trimmed routine summary for list views, built by `RoutineInfo::from_routine`.
#[derive(Debug, Serialize)]
pub struct RoutineInfo {
    /// Stable routine identifier (UUID).
    pub id: Uuid,
    /// Display name set by the user.
    pub name: String,
    /// Free-text description; may be empty.
    pub description: String,
    /// Whether the routine is allowed to fire.
    pub enabled: bool,
    /// Trigger family: `cron`, `event`, `system_event` or `manual`.
    pub trigger_type: String,
    /// Human-readable trigger, such as `cron: <schedule>` or `manual only`.
    pub trigger_summary: String,
    /// Action family: `lightweight` or `full_job`.
    pub action_type: String,
    /// RFC 3339 time of the most recent run, or `null` if it has never run.
    pub last_run_at: Option<String>,
    /// RFC 3339 time of the next scheduled fire, or `null` if none is scheduled.
    pub next_fire_at: Option<String>,
    /// Total number of runs recorded for the routine.
    pub run_count: u64,
    /// Failed runs in a row; above zero, `status` reads `failing` unless the routine is disabled.
    pub consecutive_failures: u32,
    /// Derived state: `disabled`, `failing` or `active`, checked in that order.
    pub status: String,
}

impl RoutineInfo {
    /// Convert a `Routine` to the trimmed `RoutineInfo` for list display.
    pub fn from_routine(r: &crate::agent::routine::Routine) -> Self {
        let (trigger_type, trigger_summary) = match &r.trigger {
            crate::agent::routine::Trigger::Cron { schedule, .. } => {
                ("cron".to_string(), format!("cron: {}", schedule))
            }
            crate::agent::routine::Trigger::Event {
                pattern, channel, ..
            } => {
                let ch = channel.as_deref().unwrap_or("any");
                ("event".to_string(), format!("on {} /{}/", ch, pattern))
            }
            crate::agent::routine::Trigger::SystemEvent {
                source, event_type, ..
            } => (
                "system_event".to_string(),
                format!("event: {}.{}", source, event_type),
            ),
            crate::agent::routine::Trigger::Manual => {
                ("manual".to_string(), "manual only".to_string())
            }
        };

        let action_type = match &r.action {
            crate::agent::routine::RoutineAction::Lightweight { .. } => "lightweight",
            crate::agent::routine::RoutineAction::FullJob { .. } => "full_job",
        };

        let status = if !r.enabled {
            "disabled"
        } else if r.consecutive_failures > 0 {
            "failing"
        } else {
            "active"
        };

        RoutineInfo {
            id: r.id,
            name: r.name.clone(),
            description: r.description.clone(),
            enabled: r.enabled,
            trigger_type,
            trigger_summary,
            action_type: action_type.to_string(),
            last_run_at: r.last_run_at.map(|dt| dt.to_rfc3339()),
            next_fire_at: r.next_fire_at.map(|dt| dt.to_rfc3339()),
            run_count: r.run_count,
            consecutive_failures: r.consecutive_failures,
            status: status.to_string(),
        }
    }
}

/// Response body for the routine list endpoint.
#[derive(Debug, Serialize)]
pub struct RoutineListResponse {
    /// Routines in list order, each in its trimmed `RoutineInfo` form.
    pub routines: Vec<RoutineInfo>,
}

/// Aggregate routine counts for the dashboard summary.
#[derive(Debug, Serialize)]
pub struct RoutineSummaryResponse {
    /// Number of routines in any state.
    pub total: u64,
    /// Routines with `enabled` set.
    pub enabled: u64,
    /// Routines with `enabled` cleared.
    pub disabled: u64,
    /// Routines with `consecutive_failures` above zero.
    pub failing: u64,
    /// Routines whose last run time falls on or after the start of the current UTC day.
    /// Counts routines, not individual runs.
    pub runs_today: u64,
}

/// Full routine detail, including its recent run history.
#[derive(Debug, Serialize)]
pub struct RoutineDetailResponse {
    /// Stable routine identifier (UUID).
    pub id: Uuid,
    /// Display name set by the user.
    pub name: String,
    /// Free-text description; may be empty.
    pub description: String,
    /// Whether the routine is allowed to fire.
    pub enabled: bool,
    /// Serialized trigger configuration; the JSON shape follows the `Trigger` enum.
    pub trigger: serde_json::Value,
    /// Serialized action configuration; the JSON shape follows `RoutineAction`.
    pub action: serde_json::Value,
    /// Serialized guardrail limits applied to each run.
    pub guardrails: serde_json::Value,
    /// Serialized notification settings for run outcomes.
    pub notify: serde_json::Value,
    /// RFC 3339 time of the most recent run, or `null` if it has never run.
    pub last_run_at: Option<String>,
    /// RFC 3339 time of the next scheduled fire, or `null` if none is scheduled.
    pub next_fire_at: Option<String>,
    /// Total number of runs recorded for the routine.
    pub run_count: u64,
    /// Failed runs in a row; above zero, the routine counts as failing unless it is disabled.
    pub consecutive_failures: u32,
    /// RFC 3339 time the routine was created.
    pub created_at: String,
    /// Up to 20 most recent runs, newest first.
    pub recent_runs: Vec<RoutineRunInfo>,
}

/// One execution of a routine, as shown in run history.
#[derive(Debug, Serialize)]
pub struct RoutineRunInfo {
    /// Run identifier (UUID).
    pub id: Uuid,
    /// Trigger family that started this run.
    pub trigger_type: String,
    /// RFC 3339 time the run started.
    pub started_at: String,
    /// RFC 3339 time the run finished, or `null` while it is still in progress.
    pub completed_at: Option<String>,
    /// Debug name of the run status: `Running`, `Ok`, `Attention` or `Failed`.
    pub status: String,
    /// Outcome summary text, when the run produced one.
    pub result_summary: Option<String>,
    /// LLM tokens consumed by the run, when they were recorded.
    pub tokens_used: Option<i32>,
    /// Job created by the run, if any.
    pub job_id: Option<Uuid>,
}
