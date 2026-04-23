//! Replay-based LLM provider for E2E traces.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use rust_decimal::Decimal;

use ironclaw::error::LlmError;
use ironclaw::llm::recording::{RequestHint, TraceResponse, TraceStep};
use ironclaw::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, Role, ToolCall,
    ToolCompletionRequest, ToolCompletionResponse,
};

use super::trace_types::LlmTrace;

pub(super) struct TraceLlmState {
    /// Current replay cursor, visible only to sibling support modules that
    /// assert replay diagnostics.
    ///
    /// The value is a `usize` replay cursor. It is monotonic, must not wrap,
    /// and should be mutated only while holding `TraceLlm::inner`. In normal
    /// operation it stays in `0..=steps.len()`, but `TraceLlm::next_step` may
    /// briefly increment it to `steps.len() + 1` before exhaustion is detected.
    /// Test-scale traces should never approach `usize::MAX`; treat overflow as
    /// invalid trace data rather than recovering with saturation or wraparound.
    pub(super) index: usize,
    /// Captured request messages, owned with the cursor so request recording
    /// and replay advancement remain one critical section.
    captured_requests: Vec<Vec<ChatMessage>>,
}

/// An `LlmProvider` that replays canned responses from a trace.
///
/// Steps from all turns are flattened into a single sequence at construction
/// time. The provider advances through them linearly regardless of turn
/// boundaries.
///
/// Mutable replay state is held behind one mutex so request capture and step
/// advancement stay in lockstep even if a test drives the provider from more
/// than one task.
pub struct TraceLlm {
    model_name: String,
    steps: Vec<TraceStep>,
    /// Protects `TraceLlmState::index` and `captured_requests`.
    ///
    /// Sibling modules may read this for diagnostics, but replay code should
    /// use `next_step` for read-modify-write access so cursor advancement,
    /// request capture, and exhaustion errors stay consistent. See
    /// `tests/support_unit_tests/trace_llm_tests.rs` for replay behaviour
    /// examples and `tests/support_unit_tests/trace_llm_contract_tests.rs` for
    /// the diagnostics contract.
    pub(super) inner: Mutex<TraceLlmState>,
    /// Count of request-hint mismatches observed during replay.
    ///
    /// This counter is separate from `inner` because hint validation only needs
    /// lock-free increments. Writers should use `fetch_add` with relaxed
    /// ordering for best-effort diagnostics; readers should use the
    /// diagnostics helper in `trace_provider_diagnostics.rs`. The count is
    /// expected to stay small in tests, so overflow indicates runaway replay or
    /// invalid trace data rather than a recoverable condition.
    pub(super) hint_mismatches: AtomicUsize,
}

impl TraceLlm {
    /// Create from an in-memory trace.
    pub fn from_trace(trace: LlmTrace) -> Self {
        let steps: Vec<TraceStep> = trace
            .turns
            .into_iter()
            .flat_map(|turn| turn.steps)
            .collect();
        Self {
            model_name: trace.model_name,
            steps,
            inner: Mutex::new(TraceLlmState {
                index: 0,
                captured_requests: Vec::new(),
            }),
            hint_mismatches: AtomicUsize::new(0),
        }
    }

    /// Clone of all captured request message lists.
    pub fn captured_requests(&self) -> Result<Vec<Vec<ChatMessage>>, LlmError> {
        self.lock_inner()
            .map(|inner| inner.captured_requests.clone())
    }

    /// Advance the step index and return the current step, or an error if exhausted.
    ///
    /// Before returning, applies template substitution on tool_call arguments:
    /// `{{call_id.json_path}}` is replaced with the value extracted from the
    /// tool result message whose `tool_call_id` matches `call_id`. The
    /// `json_path` is a dot-separated path into the JSON content of that tool
    /// result (e.g., `{{call_cj_1.job_id}}` extracts `.job_id` from the result
    /// of tool call `call_cj_1`).
    fn next_step(&self, messages: &[ChatMessage]) -> Result<TraceStep, LlmError> {
        let idx = {
            let mut inner = self.lock_inner()?;
            inner.captured_requests.push(messages.to_vec());
            let idx = inner.index;
            inner.index += 1;
            idx
        };

        let mut step = self
            .steps
            .get(idx)
            .ok_or_else(|| LlmError::RequestFailed {
                provider: self.model_name.clone(),
                reason: format!(
                    "TraceLlm exhausted: called {} times but only {} steps",
                    idx + 1,
                    self.steps.len()
                ),
            })?
            .clone();

        if let Some(ref hint) = step.request_hint {
            self.validate_hint(hint, messages);
        }

        if let TraceResponse::ToolCalls {
            ref mut tool_calls, ..
        } = step.response
        {
            let vars = Self::extract_tool_result_vars(messages);
            if !vars.is_empty() {
                for tool_call in tool_calls.iter_mut() {
                    Self::substitute_templates(&mut tool_call.arguments, &vars);
                }
            }
        }

        Ok(step)
    }

    fn lock_inner(&self) -> Result<std::sync::MutexGuard<'_, TraceLlmState>, LlmError> {
        self.inner.lock().map_err(|_| LlmError::RequestFailed {
            provider: self.model_name.clone(),
            reason: "TraceLlm state lock poisoned".to_string(),
        })
    }

    fn validate_hint(&self, hint: &RequestHint, messages: &[ChatMessage]) {
        if let Some(ref expected_substr) = hint.last_user_message_contains {
            let expected_substr_lc = expected_substr.to_lowercase();
            let last_user = messages
                .iter()
                .rev()
                .find(|message| matches!(message.role, Role::User));
            let matched = last_user
                .map(|message| message.content.to_lowercase().contains(&expected_substr_lc))
                .unwrap_or(false);
            if !matched {
                self.hint_mismatches.fetch_add(1, Ordering::Relaxed);
                eprintln!(
                    "[TraceLlm WARN] Request hint mismatch: expected last user message to contain {:?}, \
                     got {:?}",
                    expected_substr,
                    last_user.map(|message| &message.content),
                );
            }
        }

        if let Some(min_count) = hint.min_message_count
            && messages.len() < min_count
        {
            self.hint_mismatches.fetch_add(1, Ordering::Relaxed);
            eprintln!(
                "[TraceLlm WARN] Request hint mismatch: expected >= {} messages, got {}",
                min_count,
                messages.len(),
            );
        }
    }

    #[inline]
    fn json_scalar_to_string(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            serde_json::Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    fn extract_tool_result_vars(
        messages: &[ChatMessage],
    ) -> std::collections::HashMap<String, String> {
        let mut vars = std::collections::HashMap::new();
        for message in messages {
            if message.role != Role::Tool {
                continue;
            }
            let Some(call_id) = message.tool_call_id.as_deref() else {
                continue;
            };
            let content = Self::unwrap_tool_output(&message.content);
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
                continue;
            };
            let Some(obj) = json.as_object() else {
                continue;
            };
            for (key, value) in obj {
                Self::flatten_json_vars(&format!("{call_id}.{key}"), value, &mut vars);
            }
        }
        vars
    }

    fn flatten_json_vars(
        path: &str,
        value: &serde_json::Value,
        vars: &mut std::collections::HashMap<String, String>,
    ) {
        if let Some(string_value) = Self::json_scalar_to_string(value) {
            vars.insert(path.to_string(), string_value);
            return;
        }

        match value {
            serde_json::Value::Object(map) => {
                for (key, child) in map {
                    Self::flatten_json_vars(&format!("{path}.{key}"), child, vars);
                }
            }
            serde_json::Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    Self::flatten_json_vars(&format!("{path}.{index}"), child, vars);
                }
            }
            _ => {}
        }
    }

    fn unwrap_tool_output(content: &str) -> std::borrow::Cow<'_, str> {
        let trimmed = content.trim();
        if let Some(rest) = trimmed.strip_prefix("<tool_output")
            && let Some(tag_end) = rest.find('>')
        {
            let inner = &rest[tag_end + 1..];
            if let Some(close) = inner.rfind("</tool_output>") {
                let body = inner[..close].trim();
                return std::borrow::Cow::Borrowed(body);
            }
        }
        std::borrow::Cow::Borrowed(content)
    }

    fn substitute_templates(
        value: &mut serde_json::Value,
        vars: &std::collections::HashMap<String, String>,
    ) {
        match value {
            serde_json::Value::String(s) => {
                if s.starts_with("{{") && s.ends_with("}}") && s.matches("{{").count() == 1 {
                    let key = s[2..s.len() - 2].trim();
                    if let Some(resolved) = vars.get(key) {
                        *s = resolved.clone();
                        return;
                    }
                }

                let mut result = s.clone();
                while let Some(start) = result.find("{{") {
                    if let Some(end) = result[start..].find("}}") {
                        let end = start + end + 2;
                        let key = result[start + 2..end - 2].trim();

                        if let Some(resolved) = vars.get(key) {
                            result = format!("{}{}{}", &result[..start], resolved, &result[end..]);
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                *s = result;
            }
            serde_json::Value::Object(map) => {
                for value in map.values_mut() {
                    Self::substitute_templates(value, vars);
                }
            }
            serde_json::Value::Array(array) => {
                for value in array.iter_mut() {
                    Self::substitute_templates(value, vars);
                }
            }
            _ => {}
        }
    }
}

impl ironclaw::llm::NativeLlmProvider for TraceLlm {
    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        loop {
            let step = self.next_step(&request.messages)?;
            match step.response {
                TraceResponse::Text {
                    content,
                    input_tokens,
                    output_tokens,
                } => {
                    return Ok(CompletionResponse {
                        content,
                        input_tokens,
                        output_tokens,
                        finish_reason: FinishReason::Stop,
                        cache_read_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                    });
                }
                TraceResponse::ToolCalls { .. } => continue,
                TraceResponse::UserInput { .. } => {
                    return Err(LlmError::RequestFailed {
                        provider: self.model_name.clone(),
                        reason: "TraceLlm::complete() encountered a user_input step; \
                                 these should have been filtered out during construction"
                            .to_string(),
                    });
                }
            }
        }
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let step = self.next_step(&request.messages)?;
        match step.response {
            TraceResponse::Text {
                content,
                input_tokens,
                output_tokens,
            } => Ok(ToolCompletionResponse {
                content: Some(content),
                tool_calls: Vec::new(),
                input_tokens,
                output_tokens,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }),
            TraceResponse::ToolCalls {
                tool_calls,
                input_tokens,
                output_tokens,
            } => {
                let calls: Vec<ToolCall> = tool_calls
                    .into_iter()
                    .map(|tool_call| ToolCall {
                        id: tool_call.id,
                        name: tool_call.name,
                        arguments: tool_call.arguments,
                    })
                    .collect();
                Ok(ToolCompletionResponse {
                    content: None,
                    tool_calls: calls,
                    input_tokens,
                    output_tokens,
                    finish_reason: FinishReason::ToolUse,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                })
            }
            TraceResponse::UserInput { .. } => Err(LlmError::RequestFailed {
                provider: self.model_name.clone(),
                reason: "TraceLlm::complete_with_tools() encountered a user_input step; \
                         these should have been filtered out during construction"
                    .to_string(),
            }),
        }
    }
}
