//! Helpers for recorded flat-step traces.

use ironclaw::llm::recording::{TraceResponse, TraceStep};

use super::trace_types::LlmTrace;

impl LlmTrace {
    /// Return only the playable steps from recorded traces.
    ///
    /// Recorded flat traces are normalised into turns during deserialisation,
    /// so prefer turn-owned steps and fall back to raw steps only for manually
    /// constructed traces that have not been normalised.
    pub fn playable_steps(&self) -> Vec<&TraceStep> {
        let steps: Box<dyn Iterator<Item = &TraceStep> + '_> = if self.turns.is_empty() {
            Box::new(self.steps.iter())
        } else {
            Box::new(self.turns.iter().flat_map(|turn| turn.steps.iter()))
        };

        steps
            .filter(|step| !matches!(step.response, TraceResponse::UserInput { .. }))
            .collect()
    }
}
