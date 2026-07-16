//! Routine (scheduled/triggered automation) DTOs for the web gateway API.

use serde::Serialize;
use uuid::Uuid;

// --- Routines ---

#[derive(Debug, Serialize)]
pub struct RoutineInfo {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub trigger_type: String,
    pub trigger_summary: String,
    pub action_type: String,
    pub last_run_at: Option<String>,
    pub next_fire_at: Option<String>,
    pub run_count: u64,
    pub consecutive_failures: u32,
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

#[derive(Debug, Serialize)]
pub struct RoutineListResponse {
    pub routines: Vec<RoutineInfo>,
}

#[derive(Debug, Serialize)]
pub struct RoutineSummaryResponse {
    pub total: u64,
    pub enabled: u64,
    pub disabled: u64,
    pub failing: u64,
    pub runs_today: u64,
}

#[derive(Debug, Serialize)]
pub struct RoutineDetailResponse {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub trigger: serde_json::Value,
    pub action: serde_json::Value,
    pub guardrails: serde_json::Value,
    pub notify: serde_json::Value,
    pub last_run_at: Option<String>,
    pub next_fire_at: Option<String>,
    pub run_count: u64,
    pub consecutive_failures: u32,
    pub created_at: String,
    pub recent_runs: Vec<RoutineRunInfo>,
}

#[derive(Debug, Serialize)]
pub struct RoutineRunInfo {
    pub id: Uuid,
    pub trigger_type: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub status: String,
    pub result_summary: Option<String>,
    pub tokens_used: Option<i32>,
    pub job_id: Option<Uuid>,
}
