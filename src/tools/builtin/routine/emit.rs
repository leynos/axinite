//! Event emission tool for triggering `system_event` routines.
//!
//! This module exposes a side-effecting helper that sends structured events
//! into the routine engine. Callers should use it when a workflow needs to
//! trigger routine automation explicitly rather than waiting for cron or
//! message-driven triggers.

use super::*;

/// Tool that emits structured events into the routine engine.
pub struct EventEmitTool {
    engine: Arc<RoutineEngine>,
}

impl EventEmitTool {
    /// Create a new event-emission tool backed by the shared routine engine.
    pub fn new(engine: Arc<RoutineEngine>) -> Self {
        Self { engine }
    }
}

impl NativeTool for EventEmitTool {
    fn name(&self) -> &str {
        "event_emit"
    }

    fn description(&self) -> &str {
        "Emit a structured system event to routines with a system_event trigger. \
         Use this to trigger routines from tool workflows without waiting for cron."
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        // Emitting an event can fire system_event routines that dispatch full_jobs
        // with pre-authorized Always-gated tools — same escalation risk as routine_fire.
        ApprovalRequirement::UnlessAutoApproved
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        HostedToolEligibility::ApprovalGated
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "event_source": {
                    "type": "string",
                    "description": "Event source (e.g. 'github', 'workflow', 'tool')"
                },
                "event_type": {
                    "type": "string",
                    "description": "Event type (e.g. 'issue.opened', 'pr.ready')"
                },
                "payload": {
                    "type": "object",
                    "description": "Structured event payload"
                }
            },
            "required": ["event_source", "event_type"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let source = require_str(&params, "event_source")?;
        let event_type = require_str(&params, "event_type")?;
        let payload = params
            .get("payload")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        if !payload.is_object() {
            return Err(ToolError::InvalidParameters(
                "payload must be an object".to_string(),
            ));
        }

        let fired = self
            .engine
            .emit_system_event(source, event_type, &payload, Some(&ctx.user_id))
            .await;

        let result = serde_json::json!({
            "event_source": source,
            "event_type": event_type,
            "user_id": &ctx.user_id,
            "fired_routines": fired,
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        true
    }
}
