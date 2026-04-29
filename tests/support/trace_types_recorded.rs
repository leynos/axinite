//! Helpers for recorded flat-step traces.

use ironclaw::llm::recording::{TraceResponse, TraceStep};

use super::trace_types::LlmTrace;

impl LlmTrace {
    /// Return only the playable steps from recorded traces.
    ///
    /// Recorded flat traces are normalised into turns during deserialisation,
    /// so prefer turn-owned steps and fall back to raw steps only for manually
    /// constructed traces that have not been normalised.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use ironclaw::llm::recording::{TraceResponse, TraceStep};
    ///
    /// use crate::support::trace_types::{LlmTrace, TraceExpects, TraceTurn};
    ///
    /// fn user_input_step(content: &str) -> TraceStep {
    ///     TraceStep {
    ///         request_hint: None,
    ///         response: TraceResponse::UserInput {
    ///             content: content.to_string(),
    ///         },
    ///         expected_tool_results: Vec::new(),
    ///     }
    /// }
    ///
    /// fn text_step(content: &str) -> TraceStep {
    ///     TraceStep {
    ///         request_hint: None,
    ///         response: TraceResponse::Text {
    ///             content: content.to_string(),
    ///             input_tokens: 1,
    ///             output_tokens: 1,
    ///         },
    ///         expected_tool_results: Vec::new(),
    ///     }
    /// }
    ///
    /// let turn_owned = text_step("from turn");
    /// let trace = LlmTrace {
    ///     model_name: "recorded".to_string(),
    ///     turns: vec![TraceTurn {
    ///         user_input: "hello".to_string(),
    ///         steps: vec![user_input_step("hello"), turn_owned.clone()],
    ///         expects: TraceExpects::default(),
    ///     }],
    ///     memory_snapshot: Vec::new(),
    ///     http_exchanges: Vec::new(),
    ///     expects: TraceExpects::default(),
    ///     steps: vec![text_step("raw fallback is ignored when turns exist")],
    /// };
    ///
    /// let playable = trace.playable_steps();
    /// assert_eq!(playable.len(), 1);
    /// assert!(matches!(
    ///     playable[0].response,
    ///     TraceResponse::Text { ref content, .. } if content == "from turn"
    /// ));
    ///
    /// let raw_owned = text_step("from raw steps");
    /// let raw_trace = LlmTrace {
    ///     model_name: "manual-recorded".to_string(),
    ///     turns: Vec::new(),
    ///     memory_snapshot: Vec::new(),
    ///     http_exchanges: Vec::new(),
    ///     expects: TraceExpects::default(),
    ///     steps: vec![user_input_step("ignored"), raw_owned],
    /// };
    ///
    /// let raw_playable = raw_trace.playable_steps();
    /// assert_eq!(raw_playable.len(), 1);
    /// assert!(matches!(
    ///     raw_playable[0].response,
    ///     TraceResponse::Text { ref content, .. } if content == "from raw steps"
    /// ));
    /// ```
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
