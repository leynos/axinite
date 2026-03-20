use super::*;

pub struct RoutineUpdateTool {
    store: Arc<dyn Database>,
    engine: Arc<RoutineEngine>,
}

impl RoutineUpdateTool {
    pub fn new(store: Arc<dyn Database>, engine: Arc<RoutineEngine>) -> Self {
        Self { store, engine }
    }
}

#[async_trait]
impl Tool for RoutineUpdateTool {
    fn name(&self) -> &str {
        "routine_update"
    }

    fn description(&self) -> &str {
        "Update an existing routine. Can modify trigger, prompt, schedule, or toggle enabled state. \
         Pass the routine name and only the fields you want to change."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name of the routine to update"
                },
                "enabled": {
                    "type": "boolean",
                    "description": "Enable or disable the routine"
                },
                "prompt": {
                    "type": "string",
                    "description": "New prompt/instructions"
                },
                "schedule": {
                    "type": "string",
                    "description": "New cron schedule (for cron triggers)"
                },
                "timezone": {
                    "type": "string",
                    "description": "IANA timezone for cron schedule (e.g. 'America/New_York'). Only valid for cron triggers."
                },
                "description": {
                    "type": "string",
                    "description": "New description"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let name = require_str(&params, "name")?;

        let mut routine = self
            .store
            .get_routine_by_name(&ctx.user_id, name)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("DB error: {e}")))?
            .ok_or_else(|| ToolError::ExecutionFailed(format!("routine '{}' not found", name)))?;

        // Apply updates
        if let Some(enabled) = params.get("enabled").and_then(|v| v.as_bool()) {
            routine.enabled = enabled;
        }

        if let Some(desc) = params.get("description").and_then(|v| v.as_str()) {
            routine.description = desc.to_string();
        }

        if let Some(prompt) = params.get("prompt").and_then(|v| v.as_str()) {
            match &mut routine.action {
                RoutineAction::Lightweight { prompt: p, .. } => *p = prompt.to_string(),
                RoutineAction::FullJob { description: d, .. } => *d = prompt.to_string(),
            }
        }

        // Validate timezone param if provided
        let new_timezone = params
            .get("timezone")
            .and_then(|v| v.as_str())
            .map(|tz| {
                crate::timezone::parse_timezone(tz)
                    .map(|_| tz.to_string())
                    .ok_or_else(|| {
                        ToolError::InvalidParameters(format!("invalid IANA timezone: '{tz}'"))
                    })
            })
            .transpose()?;

        let new_schedule = params.get("schedule").and_then(|v| v.as_str());

        if new_schedule.is_some() || new_timezone.is_some() {
            // Extract existing cron fields (cloned to avoid borrow conflict)
            let existing_cron = match &routine.trigger {
                Trigger::Cron { schedule, timezone } => Some((schedule.clone(), timezone.clone())),
                _ => None,
            };

            if let Some((old_schedule, old_tz)) = existing_cron {
                let effective_schedule = new_schedule.unwrap_or(&old_schedule);
                let effective_tz = new_timezone.or(old_tz);
                // Validate
                next_cron_fire(effective_schedule, effective_tz.as_deref()).map_err(|e| {
                    ToolError::InvalidParameters(format!("invalid cron schedule: {e}"))
                })?;

                routine.trigger = Trigger::Cron {
                    schedule: effective_schedule.to_string(),
                    timezone: effective_tz.clone(),
                };
                routine.next_fire_at =
                    next_cron_fire(effective_schedule, effective_tz.as_deref()).unwrap_or(None);
            } else {
                return Err(ToolError::InvalidParameters(
                    "Cannot update schedule or timezone on a non-cron routine.".to_string(),
                ));
            }
        }

        self.store
            .update_routine(&routine)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to update: {e}")))?;

        // Refresh event cache in case trigger changed
        self.engine.refresh_event_cache().await;

        let result = serde_json::json!({
            "name": routine.name,
            "enabled": routine.enabled,
            "trigger_type": routine.trigger.type_tag(),
            "next_fire_at": routine.next_fire_at.map(|t| t.to_rfc3339()),
            "status": "updated",
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}
