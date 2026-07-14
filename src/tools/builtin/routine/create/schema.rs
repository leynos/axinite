//! JSON parameter-schema builders for the routine creation tool.

pub(super) fn object_schema(properties: serde_json::Value, required: &[&str]) -> serde_json::Value {
    let mut schema = serde_json::json!({
        "type": "object",
        "properties": properties,
    });
    if !required.is_empty() {
        schema["required"] = serde_json::json!(required);
    }
    schema
}

pub(super) fn routine_core_props() -> serde_json::Value {
    serde_json::json!({
        "name": {
            "type": "string",
            "description": "Unique name for the routine (e.g. 'daily-pr-review')"
        },
        "description": {
            "type": "string",
            "description": "What this routine does"
        },
        "trigger_type": {
            "type": "string",
            "enum": ["cron", "event", "system_event", "manual"],
            "description": "When the routine fires"
        }
    })
}

pub(super) fn trigger_props() -> serde_json::Value {
    serde_json::json!({
        "schedule": {
            "type": "string",
            "description": "Cron expression (for cron trigger). E.g. '0 9 * * MON-FRI' for weekdays at 9am. Uses 6-field cron (sec min hour day month weekday)."
        },
        "event_pattern": {
            "type": "string",
            "description": "Regex pattern to match messages (for event trigger)"
        },
        "event_channel": {
            "type": "string",
            "description": "Optional channel filter for event trigger (e.g. 'telegram')"
        },
        "event_source": {
            "type": "string",
            "description": "Event source for system_event triggers (e.g. 'github')"
        },
        "event_type": {
            "type": "string",
            "description": "Event type for system_event triggers (e.g. 'issue.opened')"
        },
        "event_filters": {
            "type": "object",
            "description": "Optional exact-match filters against payload fields for system_event triggers. Values can be strings, numbers, or booleans.",
            "additionalProperties": {
                "type": ["string", "number", "boolean"]
            }
        }
    })
}

pub(super) fn action_props() -> serde_json::Value {
    serde_json::json!({
        "prompt": {
            "type": "string",
            "description": "The prompt/instructions for the routine"
        },
        "context_paths": {
            "type": "array",
            "items": { "type": "string" },
            "description": "Workspace paths to load as context (e.g. ['context/priorities.md'])"
        },
        "action_type": {
            "type": "string",
            "enum": ["lightweight", "full_job"],
            "description": "Execution mode: 'lightweight' (single LLM call, default) or 'full_job' (multi-turn with tools)"
        },
        "cooldown_secs": {
            "type": "integer",
            "description": "Minimum seconds between fires (default: 300)"
        },
        "tool_permissions": {
            "type": "array",
            "items": { "type": "string" },
            "description": "Tool names pre-authorized for Always-approval tools in full_job mode (e.g. ['shell']). UnlessAutoApproved tools are automatically permitted in routines."
        }
    })
}

pub(super) fn notification_props() -> serde_json::Value {
    serde_json::json!({
        "notify_channel": {
            "type": "string",
            "description": "Channel to send results to (e.g. 'telegram', 'slack', 'tui'). Sets the default channel for message tool calls in routine jobs."
        },
        "notify_user": {
            "type": "string",
            "description": "User/target to notify (e.g. username, chat ID). Defaults to 'default'."
        },
        "timezone": {
            "type": "string",
            "description": "IANA timezone for cron schedule evaluation (e.g. 'America/New_York'). Defaults to UTC."
        }
    })
}
