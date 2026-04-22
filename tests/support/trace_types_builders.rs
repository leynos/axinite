//! Optional `LlmTrace` builders used by support-unit tests.

use super::trace_types::{LlmTrace, TraceExpects, TraceTurn};

impl LlmTrace {
    /// Create a trace from turns.
    pub fn new(model_name: impl Into<String>, turns: Vec<TraceTurn>) -> Self {
        Self {
            model_name: model_name.into(),
            turns,
            memory_snapshot: Vec::new(),
            http_exchanges: Vec::new(),
            expects: TraceExpects::default(),
            steps: Vec::new(),
        }
    }
}
