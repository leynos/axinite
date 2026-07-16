//! Routine creation tool: parameter schema builders and routine
//! construction from tool arguments.

use super::*;

mod schema;

use schema::{action_props, notification_props, object_schema, routine_core_props, trigger_props};

fn parse_event_filters(
    params: &serde_json::Value,
) -> Result<std::collections::HashMap<String, String>, ToolError> {
    match params.get("event_filters") {
        None | Some(serde_json::Value::Null) => Ok(std::collections::HashMap::new()),
        Some(serde_json::Value::Object(obj)) => obj
            .iter()
            .map(|(key, value)| {
                crate::agent::routine::json_value_as_filter_string(value)
                    .map(|string| (key.to_string(), string))
                    .ok_or_else(|| {
                        ToolError::InvalidParameters(format!(
                            "event_filters['{}'] must be a string, number, or boolean",
                            key
                        ))
                    })
            })
            .collect(),
        Some(_) => Err(ToolError::InvalidParameters(
            "event_filters must be an object".to_string(),
        )),
    }
}

pub struct RoutineCreateTool {
    store: Arc<dyn Database>,
    engine: Arc<RoutineEngine>,
}

impl RoutineCreateTool {
    pub fn new(store: Arc<dyn Database>, engine: Arc<RoutineEngine>) -> Self {
        Self { store, engine }
    }
}

impl NativeTool for RoutineCreateTool {
    fn name(&self) -> &str {
        "routine_create"
    }

    fn description(&self) -> &str {
        "Create a new routine (scheduled or event-driven task). \
         Supports cron schedules, event pattern matching, system events, and manual triggers. \
         Use this when the user wants something to happen periodically or reactively."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let routine_core_props = routine_core_props();
        let trigger_props = trigger_props();
        let action_props = action_props();
        let notification_props = notification_props();

        object_schema(
            serde_json::json!({
                "name": routine_core_props["name"],
                "description": routine_core_props["description"],
                "trigger_type": routine_core_props["trigger_type"],
                "schedule": trigger_props["schedule"],
                "event_pattern": trigger_props["event_pattern"],
                "event_channel": trigger_props["event_channel"],
                "event_source": trigger_props["event_source"],
                "event_type": trigger_props["event_type"],
                "event_filters": trigger_props["event_filters"],
                "prompt": action_props["prompt"],
                "context_paths": action_props["context_paths"],
                "action_type": action_props["action_type"],
                "cooldown_secs": action_props["cooldown_secs"],
                "tool_permissions": action_props["tool_permissions"],
                "notify_channel": notification_props["notify_channel"],
                "notify_user": notification_props["notify_user"],
                "timezone": notification_props["timezone"]
            }),
            &["name", "trigger_type", "prompt"],
        )
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let name = require_str(&params, "name")?;

        let description = params
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let trigger_type = require_str(&params, "trigger_type")?;

        let prompt = require_str(&params, "prompt")?;

        // Build trigger
        let trigger = match trigger_type {
            "cron" => {
                let schedule =
                    params
                        .get("schedule")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            ToolError::InvalidParameters(
                                "cron trigger requires 'schedule'".to_string(),
                            )
                        })?;
                let timezone = params
                    .get("timezone")
                    .and_then(|v| v.as_str())
                    .map(|tz| {
                        crate::timezone::parse_timezone(tz)
                            .map(|_| tz.to_string())
                            .ok_or_else(|| {
                                ToolError::InvalidParameters(format!(
                                    "invalid IANA timezone: '{tz}'"
                                ))
                            })
                    })
                    .transpose()?;
                // Validate cron expression
                next_cron_fire(schedule, timezone.as_deref()).map_err(|e| {
                    ToolError::InvalidParameters(format!("invalid cron schedule: {e}"))
                })?;
                Trigger::Cron {
                    schedule: schedule.to_string(),
                    timezone,
                }
            }
            "event" => {
                let pattern = params
                    .get("event_pattern")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidParameters(
                            "event trigger requires 'event_pattern'".to_string(),
                        )
                    })?;
                // Validate regex
                regex::Regex::new(pattern)
                    .map_err(|e| ToolError::InvalidParameters(format!("invalid regex: {e}")))?;
                let channel = params
                    .get("event_channel")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                Trigger::Event {
                    channel,
                    pattern: pattern.to_string(),
                }
            }
            "system_event" => {
                let source = params
                    .get("event_source")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidParameters(
                            "system_event trigger requires 'event_source'".to_string(),
                        )
                    })?;
                let event_type = params
                    .get("event_type")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        ToolError::InvalidParameters(
                            "system_event trigger requires 'event_type'".to_string(),
                        )
                    })?;
                let filters = parse_event_filters(&params)?;
                Trigger::SystemEvent {
                    source: source.to_string(),
                    event_type: event_type.to_string(),
                    filters,
                }
            }
            "manual" => Trigger::Manual,
            other => {
                return Err(ToolError::InvalidParameters(format!(
                    "unknown trigger_type: {other}"
                )));
            }
        };

        // Build action
        let action_type = params
            .get("action_type")
            .and_then(|v| v.as_str())
            .unwrap_or("lightweight");

        let context_paths: Vec<String> = params
            .get("context_paths")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let action = match action_type {
            "lightweight" => RoutineAction::Lightweight {
                prompt: prompt.to_string(),
                context_paths,
                max_tokens: 4096,
            },
            "full_job" => {
                let tool_permissions = crate::agent::routine::parse_tool_permissions(&params);
                RoutineAction::FullJob {
                    title: name.to_string(),
                    description: prompt.to_string(),
                    max_iterations: 10,
                    tool_permissions,
                }
            }
            other => {
                return Err(ToolError::InvalidParameters(format!(
                    "unknown action_type: {other}"
                )));
            }
        };

        let cooldown_secs = params
            .get("cooldown_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(300);

        // Compute next fire time for cron
        let next_fire = if let Trigger::Cron {
            ref schedule,
            ref timezone,
        } = trigger
        {
            next_cron_fire(schedule, timezone.as_deref()).unwrap_or(None)
        } else {
            None
        };

        let routine = Routine {
            id: Uuid::new_v4(),
            name: name.to_string(),
            description: description.to_string(),
            user_id: ctx.user_id.clone(),
            enabled: true,
            trigger,
            action,
            guardrails: RoutineGuardrails {
                cooldown: Duration::from_secs(cooldown_secs),
                max_concurrent: 1,
                dedup_window: None,
            },
            notify: NotifyConfig {
                channel: params
                    .get("notify_channel")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                user: params
                    .get("notify_user")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default")
                    .to_string(),
                ..NotifyConfig::default()
            },
            last_run_at: None,
            next_fire_at: next_fire,
            run_count: 0,
            consecutive_failures: 0,
            state: serde_json::json!({}),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        self.store
            .create_routine(&routine)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to create routine: {e}")))?;

        // Refresh event cache if this is an event trigger
        if matches!(
            routine.trigger,
            Trigger::Event { .. } | Trigger::SystemEvent { .. }
        ) {
            self.engine.refresh_event_cache().await;
        }

        let result = serde_json::json!({
            "id": routine.id.to_string(),
            "name": routine.name,
            "trigger_type": routine.trigger.type_tag(),
            "next_fire_at": routine.next_fire_at.map(|t| t.to_rfc3339()),
            "status": "created",
        });

        Ok(ToolOutput::success(result, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}
