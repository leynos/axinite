//! Trace data types and JSON loading helpers for replay-based LLM tests.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use ironclaw::llm::recording::{HttpExchange, MemorySnapshotEntry, TraceResponse, TraceStep};

/// A single turn in a trace: one user message and the LLM response steps that follow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceTurn {
    pub user_input: String,
    pub steps: Vec<TraceStep>,
    /// Declarative expectations for this turn (optional).
    #[serde(default, skip_serializing_if = "TraceExpects::is_empty")]
    pub expects: TraceExpects,
}

/// A complete LLM trace: a model name and an ordered list of turns.
///
/// Each turn pairs a user message with the LLM response steps that follow it.
/// For JSON backward compatibility, traces with a flat top-level `"steps"` array
/// (no `"turns"`) are deserialized into turns by splitting at `UserInput` boundaries.
///
/// Recorded traces (from `RecordingLlm`) may also include `memory_snapshot`,
/// `http_exchanges`, and `user_input` response steps.
#[derive(Debug, Clone, Serialize)]
pub struct LlmTrace {
    pub model_name: String,
    pub turns: Vec<TraceTurn>,
    /// Workspace memory documents captured before the recording session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_snapshot: Vec<MemorySnapshotEntry>,
    /// HTTP exchanges recorded during the session, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub http_exchanges: Vec<HttpExchange>,
    /// Declarative expectations for the whole trace (optional).
    #[serde(default, skip_serializing_if = "TraceExpects::is_empty")]
    pub expects: TraceExpects,
    /// Raw steps before turn conversion (populated only for recorded traces).
    /// Used by `playable_steps()` for recorded-format inspection.
    #[serde(skip)]
    pub steps: Vec<TraceStep>,
}

/// Declarative expectations for a trace or turn.
///
/// All fields are optional and default to empty/None, so traces without
/// `expects` work unchanged (backward compatible).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceExpects {
    /// Each string must appear in the response (case-insensitive).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_contains: Vec<String>,
    /// None of these may appear in the response (case-insensitive).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_not_contains: Vec<String>,
    /// Regex that must match the response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_matches: Option<String>,
    /// Each tool name must appear in started calls.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_used: Vec<String>,
    /// None of these tool names may appear.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_not_used: Vec<String>,
    /// If true, all tools must succeed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_tools_succeeded: Option<bool>,
    /// Upper bound on tool call count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<usize>,
    /// Minimum response count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_responses: Option<usize>,
    /// Tool result preview must contain substring (tool_name -> substring).
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub tool_results_contain: HashMap<String, String>,
    /// Tools must have been called in this relative order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools_order: Vec<String>,
}

impl TraceExpects {
    /// Returns true when no expectations are specified.
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

/// Raw deserialization helper -- accepts either `turns` or flat `steps`.
#[derive(Deserialize)]
struct RawLlmTrace {
    model_name: String,
    #[serde(default)]
    steps: Vec<TraceStep>,
    #[serde(default)]
    turns: Vec<TraceTurn>,
    #[serde(default)]
    memory_snapshot: Vec<MemorySnapshotEntry>,
    #[serde(default)]
    http_exchanges: Vec<HttpExchange>,
    #[serde(default)]
    expects: TraceExpects,
}

impl<'de> Deserialize<'de> for LlmTrace {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawLlmTrace::deserialize(deserializer)?;
        let RawLlmTrace {
            model_name,
            steps,
            turns,
            memory_snapshot,
            http_exchanges,
            expects,
        } = raw;
        let raw_steps = if turns.is_empty() {
            steps.clone()
        } else {
            Vec::new()
        };
        let turns = if !turns.is_empty() {
            turns
        } else if !steps.is_empty() {
            let mut turns = Vec::new();
            let mut current_input = "(test input)".to_string();
            let mut current_steps: Vec<TraceStep> = Vec::new();

            for step in steps {
                if let TraceResponse::UserInput { ref content } = step.response {
                    if !current_steps.is_empty() {
                        turns.push(TraceTurn {
                            user_input: current_input.clone(),
                            steps: std::mem::take(&mut current_steps),
                            expects: TraceExpects::default(),
                        });
                    }
                    current_input = content.clone();
                } else {
                    current_steps.push(step);
                }
            }

            if !current_steps.is_empty() {
                turns.push(TraceTurn {
                    user_input: current_input,
                    steps: current_steps,
                    expects: TraceExpects::default(),
                });
            }

            turns
        } else {
            vec![]
        };

        Ok(LlmTrace {
            model_name,
            turns,
            memory_snapshot,
            http_exchanges,
            expects,
            steps: raw_steps,
        })
    }
}

impl LlmTrace {}

/// Load a trace from JSON, applying a structured mutation before deserializing.
pub async fn load_trace_with_mutation<F>(
    path: impl AsRef<std::path::Path>,
    mutate: F,
) -> anyhow::Result<LlmTrace>
where
    F: FnOnce(&mut serde_json::Value),
{
    let contents = tokio::fs::read_to_string(path).await?;
    let mut value: serde_json::Value = serde_json::from_str(&contents)?;
    mutate(&mut value);
    let trace = serde_json::from_value(value)?;
    Ok(trace)
}
