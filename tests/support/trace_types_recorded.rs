//! Helpers for recorded flat-step traces.

use ironclaw::llm::recording::{TraceResponse, TraceStep};

use super::trace_types::LlmTrace;

impl LlmTrace {
    /// Return only the playable steps from the raw steps (text + tool_calls),
    /// skipping `user_input` markers. Only meaningful for recorded traces that
    /// were deserialized from a flat `steps` array.
    pub fn playable_steps(&self) -> Vec<&TraceStep> {
        self.steps
            .iter()
            .filter(|step| !matches!(step.response, TraceResponse::UserInput { .. }))
            .collect()
    }
}
