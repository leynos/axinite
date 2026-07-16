//! Routine trigger types: cron, event, system-event and manual triggers,
//! plus cron schedule evaluation helpers.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::RoutineError;

use super::required_str_field;

/// When a routine should fire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// Fire on a cron schedule (e.g. "0 9 * * MON-FRI" or "every 2h").
    Cron {
        schedule: String,
        #[serde(default)]
        timezone: Option<String>,
    },
    /// Fire when a channel message matches a pattern.
    Event {
        /// Optional channel filter (e.g. "telegram", "slack").
        channel: Option<String>,
        /// Regex pattern to match against message content.
        pattern: String,
    },
    /// Fire when a structured system event is emitted.
    SystemEvent {
        /// Event source namespace (e.g. "github", "workflow", "tool").
        source: String,
        /// Event type within the source (e.g. "issue.opened").
        event_type: String,
        /// Optional exact-match filters against payload top-level fields.
        #[serde(default)]
        filters: std::collections::HashMap<String, String>,
    },
    /// Only fires via tool call or CLI.
    Manual,
}

impl Trigger {
    /// The string tag stored in the DB trigger_type column.
    pub fn type_tag(&self) -> &'static str {
        match self {
            Trigger::Cron { .. } => "cron",
            Trigger::Event { .. } => "event",
            Trigger::SystemEvent { .. } => "system_event",
            Trigger::Manual => "manual",
        }
    }

    /// Parse a trigger from its DB representation.
    pub fn from_db(trigger_type: &str, config: serde_json::Value) -> Result<Self, RoutineError> {
        match trigger_type {
            "cron" => Self::cron_from_config(&config),
            "event" => Self::event_from_config(&config),
            "system_event" => Self::system_event_from_config(&config),
            "manual" => Ok(Trigger::Manual),
            other => Err(RoutineError::UnknownTriggerType {
                trigger_type: other.to_string(),
            }),
        }
    }

    /// Parse a `cron` trigger's config, discarding invalid timezones.
    fn cron_from_config(config: &serde_json::Value) -> Result<Self, RoutineError> {
        let schedule = required_str_field(config, "cron trigger", "schedule")?;
        let timezone = config
            .get("timezone")
            .and_then(|v| v.as_str())
            .and_then(|tz| {
                if crate::timezone::parse_timezone(tz).is_some() {
                    Some(tz.to_string())
                } else {
                    tracing::warn!(
                        "Ignoring invalid timezone '{}' from DB for cron trigger",
                        tz
                    );
                    None
                }
            });
        Ok(Trigger::Cron { schedule, timezone })
    }

    /// Parse an `event` trigger's config.
    fn event_from_config(config: &serde_json::Value) -> Result<Self, RoutineError> {
        let pattern = required_str_field(config, "event trigger", "pattern")?;
        let channel = config
            .get("channel")
            .and_then(|v| v.as_str())
            .map(String::from);
        Ok(Trigger::Event { channel, pattern })
    }

    /// Parse a `system_event` trigger's config, skipping non-scalar filters.
    fn system_event_from_config(config: &serde_json::Value) -> Result<Self, RoutineError> {
        let source = required_str_field(config, "system_event trigger", "source")?;
        let event_type = required_str_field(config, "system_event trigger", "event_type")?;
        let filters = config
            .get("filters")
            .and_then(|v| v.as_object())
            .map(|m| {
                m.iter()
                    .filter_map(|(k, v)| json_value_as_filter_string(v).map(|s| (k.clone(), s)))
                    .collect()
            })
            .unwrap_or_default();
        Ok(Trigger::SystemEvent {
            source,
            event_type,
            filters,
        })
    }

    /// Serialize trigger-specific config to JSON for DB storage.
    pub fn to_config_json(&self) -> serde_json::Value {
        match self {
            Trigger::Cron { schedule, timezone } => serde_json::json!({
                "schedule": schedule,
                "timezone": timezone,
            }),
            Trigger::Event { channel, pattern } => serde_json::json!({
                "pattern": pattern,
                "channel": channel,
            }),
            Trigger::SystemEvent {
                source,
                event_type,
                filters,
            } => serde_json::json!({
                "source": source,
                "event_type": event_type,
                "filters": filters,
            }),
            Trigger::Manual => serde_json::json!({}),
        }
    }
}

/// Convert a JSON value to a string for filter storage.
///
/// Handles strings, numbers, and booleans — consistent with the matching
/// logic in `routine_engine::json_value_as_string`.
pub fn json_value_as_filter_string(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Parse a cron expression and compute the next fire time from now.
///
/// When `timezone` is provided and valid, the schedule is evaluated in that
/// timezone and the result is converted back to UTC. Otherwise UTC is used.
pub fn next_cron_fire(
    schedule: &str,
    timezone: Option<&str>,
) -> Result<Option<DateTime<Utc>>, RoutineError> {
    let cron_schedule =
        cron::Schedule::from_str(schedule).map_err(|e| RoutineError::InvalidCron {
            reason: e.to_string(),
        })?;
    if let Some(tz) = timezone.and_then(crate::timezone::parse_timezone) {
        Ok(cron_schedule
            .upcoming(tz)
            .next()
            .map(|dt| dt.with_timezone(&Utc)))
    } else {
        Ok(cron_schedule.upcoming(Utc).next())
    }
}
