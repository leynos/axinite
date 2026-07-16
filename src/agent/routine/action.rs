//! Routine action types: lightweight LLM calls and full worker jobs.

use serde::{Deserialize, Serialize};

use crate::error::RoutineError;

use super::required_str_field;

/// What happens when a routine fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoutineAction {
    /// Single LLM call, no tools. Cheap and fast.
    Lightweight {
        /// The prompt sent to the LLM.
        prompt: String,
        /// Workspace paths to load as context (e.g. ["context/priorities.md"]).
        #[serde(default)]
        context_paths: Vec<String>,
        /// Max output tokens (default: 4096).
        #[serde(default = "default_max_tokens")]
        max_tokens: u32,
    },
    /// Full multi-turn worker job with tool access.
    FullJob {
        /// Job title for the scheduler.
        title: String,
        /// Job description / initial prompt.
        description: String,
        /// Max reasoning iterations (default: 10).
        #[serde(default = "default_max_iterations")]
        max_iterations: u32,
        /// Tool names pre-authorized for `Always`-approval tools (e.g. destructive
        /// shell commands, cross-channel messaging). `UnlessAutoApproved` tools are
        /// automatically permitted in routine jobs without listing them here.
        #[serde(default)]
        tool_permissions: Vec<String>,
    },
}

fn default_max_tokens() -> u32 {
    4096
}

fn default_max_iterations() -> u32 {
    10
}

/// Parse a `tool_permissions` JSON array into a `Vec<String>`.
pub fn parse_tool_permissions(value: &serde_json::Value) -> Vec<String> {
    value
        .get("tool_permissions")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

impl RoutineAction {
    /// The string tag stored in the DB action_type column.
    pub fn type_tag(&self) -> &'static str {
        match self {
            RoutineAction::Lightweight { .. } => "lightweight",
            RoutineAction::FullJob { .. } => "full_job",
        }
    }

    /// Parse an action from its DB representation.
    pub fn from_db(action_type: &str, config: serde_json::Value) -> Result<Self, RoutineError> {
        match action_type {
            "lightweight" => Self::lightweight_from_config(&config),
            "full_job" => Self::full_job_from_config(&config),
            other => Err(RoutineError::UnknownActionType {
                action_type: other.to_string(),
            }),
        }
    }

    /// Parse a `lightweight` action's config, defaulting optional fields.
    fn lightweight_from_config(config: &serde_json::Value) -> Result<Self, RoutineError> {
        let prompt = required_str_field(config, "lightweight action", "prompt")?;
        let context_paths = config
            .get("context_paths")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let max_tokens = config
            .get("max_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(default_max_tokens() as u64) as u32;
        Ok(RoutineAction::Lightweight {
            prompt,
            context_paths,
            max_tokens,
        })
    }

    /// Parse a `full_job` action's config, defaulting optional fields.
    fn full_job_from_config(config: &serde_json::Value) -> Result<Self, RoutineError> {
        let title = required_str_field(config, "full_job action", "title")?;
        let description = required_str_field(config, "full_job action", "description")?;
        let max_iterations = config
            .get("max_iterations")
            .and_then(|v| v.as_u64())
            .unwrap_or(default_max_iterations() as u64) as u32;
        let tool_permissions = parse_tool_permissions(config);
        Ok(RoutineAction::FullJob {
            title,
            description,
            max_iterations,
            tool_permissions,
        })
    }

    /// Serialize action config to JSON for DB storage.
    pub fn to_config_json(&self) -> serde_json::Value {
        match self {
            RoutineAction::Lightweight {
                prompt,
                context_paths,
                max_tokens,
            } => serde_json::json!({
                "prompt": prompt,
                "context_paths": context_paths,
                "max_tokens": max_tokens,
            }),
            RoutineAction::FullJob {
                title,
                description,
                max_iterations,
                tool_permissions,
            } => serde_json::json!({
                "title": title,
                "description": description,
                "max_iterations": max_iterations,
                "tool_permissions": tool_permissions,
            }),
        }
    }
}
