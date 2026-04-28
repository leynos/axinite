//! Core trace-construction and loading helpers used by replay-based tests.

use std::path::Path;

use anyhow::Result;
use ironclaw::llm::recording::TraceStep;

use super::trace_types::{LlmTrace, TraceExpects, TraceTurn};

impl LlmTrace {
    /// Convenience: create a single-turn trace (for simple tests).
    pub fn single_turn(
        model_name: impl Into<String>,
        user_input: impl Into<String>,
        steps: Vec<TraceStep>,
    ) -> Self {
        Self {
            model_name: model_name.into(),
            turns: vec![TraceTurn {
                user_input: user_input.into(),
                steps,
                expects: TraceExpects::default(),
            }],
            memory_snapshot: Vec::new(),
            http_exchanges: Vec::new(),
            expects: TraceExpects::default(),
            steps: Vec::new(),
        }
    }

    /// Load a trace from a JSON file asynchronously.
    pub async fn from_file_async(path: impl AsRef<Path>) -> Result<Self> {
        let contents = tokio::fs::read_to_string(path).await?;
        let trace: Self = serde_json::from_str(&contents)?;
        Ok(trace)
    }
}
