//! Path-mutation helpers for trace fixtures used by end-to-end tests.

use ironclaw::llm::recording::{TraceResponse, TraceStep};

use super::trace_types::LlmTrace;

impl LlmTrace {
    /// Replace all occurrences of `from` with `to` in tool call arguments.
    ///
    /// Walks through all turns and steps, patching any string values in tool call
    /// arguments that contain the `from` path. Useful for adapting recorded traces
    /// to use test-specific temporary directories.
    ///
    /// ```rust
    /// # fn load_trace_fixture() -> crate::support::trace_types::LlmTrace {
    /// #     use ironclaw::llm::recording::{TraceResponse, TraceStep, TraceToolCall};
    /// #     crate::support::trace_types::LlmTrace::single_turn(
    /// #         "example-model",
    /// #         "patch the path",
    /// #         vec![TraceStep {
    /// #             request_hint: None,
    /// #             response: TraceResponse::ToolCalls {
    /// #                 tool_calls: vec![TraceToolCall {
    /// #                     id: "call-1".to_string(),
    /// #                     name: "write_file".to_string(),
    /// #                     arguments: serde_json::json!({
    /// #                         "path": "/tmp/run-123/output.txt"
    /// #                     }),
    /// #                 }],
    /// #                 input_tokens: 1,
    /// #                 output_tokens: 1,
    /// #             },
    /// #             expected_tool_results: Vec::new(),
    /// #         }],
    /// #     )
    /// # }
    /// let mut trace = load_trace_fixture();
    /// let patched = trace.patch_path("/tmp/run-123", "/tmp/test-run");
    ///
    /// assert!(patched > 0);
    /// ```
    pub fn patch_path(&mut self, from: &str, to: &str) -> usize {
        if from.is_empty() || from == to {
            return 0;
        }

        let mut patched = 0;
        for turn in &mut self.turns {
            patched += patch_steps(&mut turn.steps, from, to);
        }
        patched += patch_steps(&mut self.steps, from, to);
        patched
    }
}

fn patch_steps(steps: &mut [TraceStep], from: &str, to: &str) -> usize {
    let mut patched = 0;
    for step in steps {
        patched += patch_tool_calls_in_step(step, from, to);
    }
    patched
}

fn patch_tool_calls_in_step(step: &mut TraceStep, from: &str, to: &str) -> usize {
    let TraceResponse::ToolCalls { tool_calls, .. } = &mut step.response else {
        return 0;
    };

    let mut patched = 0;
    for call in tool_calls {
        if patch_json_value(&mut call.arguments, from, to) {
            patched += 1;
        }
    }
    patched
}

fn patch_json_value(value: &mut serde_json::Value, from: &str, to: &str) -> bool {
    match value {
        serde_json::Value::String(s) if s.contains(from) => {
            *s = s.replace(from, to);
            true
        }
        serde_json::Value::Array(items) => {
            let mut mutated = false;
            for item in items {
                mutated |= patch_json_value(item, from, to);
            }
            mutated
        }
        serde_json::Value::Object(map) => {
            let mut mutated = false;
            for child in map.values_mut() {
                mutated |= patch_json_value(child, from, to);
            }
            mutated
        }
        _ => false,
    }
}
