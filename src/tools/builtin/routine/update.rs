use super::*;

struct UpdatePatch {
    name: String,
    enabled: Option<bool>,
    prompt: Option<String>,
    schedule: Option<String>,
    timezone: Option<String>,
    description: Option<String>,
}

impl UpdatePatch {
    fn is_noop(&self) -> bool {
        self.enabled.is_none()
            && self.prompt.is_none()
            && self.schedule.is_none()
            && self.timezone.is_none()
            && self.description.is_none()
    }
}

fn parse_update_patch(params: &serde_json::Value) -> Result<UpdatePatch, ToolError> {
    let name = require_str(params, "name")?.to_string();
    let timezone = params
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

    Ok(UpdatePatch {
        name,
        enabled: params.get("enabled").and_then(|v| v.as_bool()),
        prompt: params
            .get("prompt")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
        schedule: params
            .get("schedule")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
        timezone,
        description: params
            .get("description")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
    })
}

async fn load_and_authorize(
    store: &Arc<dyn Database>,
    name: &str,
    user: &str,
) -> Result<Routine, ToolError> {
    store
        .get_routine_by_name(user, name)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("DB error: {e}")))?
        .ok_or_else(|| ToolError::ExecutionFailed(format!("routine '{}' not found", name)))
}

fn is_invalid_cron(schedule: &str, timezone: Option<&str>) -> bool {
    next_cron_fire(schedule, timezone).is_err()
}

fn validate_patch(patch: &UpdatePatch, routine: &Routine) -> Result<(), ToolError> {
    if patch.is_noop() {
        return Ok(());
    }

    let has_schedule_update = patch.schedule.is_some() || patch.timezone.is_some();
    if !has_schedule_update {
        return Ok(());
    }

    let Trigger::Cron {
        schedule: old_schedule,
        timezone: old_timezone,
    } = &routine.trigger
    else {
        return Err(ToolError::InvalidParameters(
            "Cannot update schedule or timezone on a non-cron routine.".to_string(),
        ));
    };

    let effective_schedule = patch.schedule.as_deref().unwrap_or(old_schedule);
    let effective_timezone = patch.timezone.as_deref().or(old_timezone.as_deref());
    if is_invalid_cron(effective_schedule, effective_timezone) {
        let Err(err) = next_cron_fire(effective_schedule, effective_timezone) else {
            return Ok(());
        };
        return Err(ToolError::InvalidParameters(format!(
            "invalid cron schedule: {err}"
        )));
    }

    Ok(())
}

fn apply_schedule_patch(routine: &mut Routine, patch: &UpdatePatch) {
    let (old_schedule, old_timezone) = match &routine.trigger {
        Trigger::Cron { schedule, timezone } => (schedule.clone(), timezone.clone()),
        _ => return,
    };
    let effective_schedule = patch.schedule.as_deref().unwrap_or(&old_schedule);
    let effective_timezone = patch.timezone.clone().or(old_timezone);

    routine.trigger = Trigger::Cron {
        schedule: effective_schedule.to_string(),
        timezone: effective_timezone.clone(),
    };
    routine.next_fire_at =
        next_cron_fire(effective_schedule, effective_timezone.as_deref()).unwrap_or(None);
}

fn apply_patch(routine: &mut Routine, patch: &UpdatePatch) {
    if let Some(enabled) = patch.enabled {
        routine.enabled = enabled;
    }

    if let Some(description) = &patch.description {
        routine.description = description.clone();
    }

    if let Some(prompt) = &patch.prompt {
        match &mut routine.action {
            RoutineAction::Lightweight {
                prompt: existing, ..
            } => *existing = prompt.clone(),
            RoutineAction::FullJob { description, .. } => *description = prompt.clone(),
        }
    }

    if patch.schedule.is_some() || patch.timezone.is_some() {
        apply_schedule_patch(routine, patch);
    }
}

async fn persist_and_output(
    store: &Arc<dyn Database>,
    engine: &Arc<RoutineEngine>,
    routine: Routine,
    elapsed: Duration,
) -> Result<ToolOutput, ToolError> {
    store
        .update_routine(&routine)
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("failed to update: {e}")))?;

    engine.refresh_event_cache().await;

    let result = serde_json::json!({
        "name": routine.name,
        "enabled": routine.enabled,
        "trigger_type": routine.trigger.type_tag(),
        "next_fire_at": routine.next_fire_at.map(|t| t.to_rfc3339()),
        "status": "updated",
    });

    Ok(ToolOutput::success(result, elapsed))
}

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
        let patch = parse_update_patch(&params)?;
        let mut routine = load_and_authorize(&self.store, &patch.name, &ctx.user_id).await?;
        validate_patch(&patch, &routine)?;
        apply_patch(&mut routine, &patch);
        persist_and_output(&self.store, &self.engine, routine, start.elapsed()).await
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}
