//! Turn model: a single request/response pair within a thread, including
//! recorded tool calls and indexed tool-call mutation errors.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors for indexed tool-call mutations on a turn.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ToolCallIndexError {
    #[error("tool call index {idx} out of bounds (len={len})")]
    OutOfBounds { idx: usize, len: usize },
}

/// State of a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnState {
    /// Turn is being processed.
    Processing,
    /// Turn completed successfully.
    Completed,
    /// Turn failed with an error.
    Failed,
    /// Turn was interrupted.
    Interrupted,
}

/// A single turn (request/response pair) in a thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    /// Turn number (0-indexed).
    pub turn_number: usize,
    /// User input that started this turn.
    pub user_input: String,
    /// Agent response (if completed).
    pub response: Option<String>,
    /// Tool calls made during this turn.
    pub tool_calls: Vec<TurnToolCall>,
    /// Turn state.
    pub state: TurnState,
    /// When the turn started.
    pub started_at: DateTime<Utc>,
    /// When the turn completed.
    pub completed_at: Option<DateTime<Utc>>,
    /// Error message (if failed).
    pub error: Option<String>,
    /// Transient image content parts for multimodal LLM input.
    /// Not serialized — images are only needed for the current LLM call.
    /// The text description in `user_input` persists for compaction/context.
    #[serde(skip)]
    pub image_content_parts: Vec<crate::llm::ContentPart>,
}

impl Turn {
    fn set_tool_outcome_at(
        &mut self,
        idx: usize,
        result: Option<serde_json::Value>,
        error: Option<String>,
    ) -> Result<(), ToolCallIndexError> {
        let len = self.tool_calls.len();
        let tool_call = self
            .tool_calls
            .get_mut(idx)
            .ok_or(ToolCallIndexError::OutOfBounds { idx, len })?;
        tool_call.result = result;
        tool_call.error = error;
        Ok(())
    }

    /// Create a new turn.
    pub fn new(turn_number: usize, user_input: impl Into<String>) -> Self {
        Self {
            turn_number,
            user_input: user_input.into(),
            response: None,
            tool_calls: Vec::new(),
            state: TurnState::Processing,
            started_at: Utc::now(),
            completed_at: None,
            error: None,
            image_content_parts: Vec::new(),
        }
    }

    /// Complete this turn.
    pub fn complete(&mut self, response: impl Into<String>) {
        self.response = Some(response.into());
        self.state = TurnState::Completed;
        self.completed_at = Some(Utc::now());
        // Free image data — only needed for the initial LLM call, not subsequent turns
        self.image_content_parts.clear();
    }

    /// Fail this turn.
    pub fn fail(&mut self, error: impl Into<String>) {
        self.error = Some(error.into());
        self.state = TurnState::Failed;
        self.completed_at = Some(Utc::now());
        self.image_content_parts.clear();
    }

    /// Interrupt this turn.
    pub fn interrupt(&mut self) {
        self.state = TurnState::Interrupted;
        self.completed_at = Some(Utc::now());
        self.image_content_parts.clear();
    }

    /// Record a tool call.
    pub fn record_tool_call(&mut self, name: impl Into<String>, params: serde_json::Value) {
        self.tool_calls.push(TurnToolCall {
            name: name.into(),
            parameters: params,
            result: None,
            error: None,
        });
    }

    /// Record tool call result.
    pub fn record_tool_result(&mut self, result: serde_json::Value) {
        if let Some(call) = self.tool_calls.last_mut() {
            call.result = Some(result);
        }
    }

    /// Record tool call result for a specific tool-call slot.
    pub fn record_tool_result_at(
        &mut self,
        idx: usize,
        result: serde_json::Value,
    ) -> Result<(), ToolCallIndexError> {
        self.set_tool_outcome_at(idx, Some(result), None)
    }

    fn parse_tool_result(result_content: &str) -> serde_json::Value {
        let trimmed = result_content.trim_start();
        if matches!(trimmed.as_bytes().first(), Some(b'{' | b'[')) {
            serde_json::from_str(result_content)
                .unwrap_or_else(|_| serde_json::Value::String(result_content.to_string()))
        } else {
            serde_json::Value::String(result_content.to_string())
        }
    }

    /// Record tool call result, parsing structured JSON where possible.
    pub fn record_tool_result_content(&mut self, result_content: &str) {
        self.record_tool_result(Self::parse_tool_result(result_content));
    }

    /// Record tool call result for a specific slot, parsing structured JSON
    /// where possible.
    pub fn record_tool_result_content_at(
        &mut self,
        idx: usize,
        result_content: &str,
    ) -> Result<(), ToolCallIndexError> {
        self.record_tool_result_at(idx, Self::parse_tool_result(result_content))
    }

    /// Record tool call error.
    pub fn record_tool_error(&mut self, error: impl Into<String>) {
        if let Some(call) = self.tool_calls.last_mut() {
            call.error = Some(error.into());
        }
    }

    /// Record tool call error for a specific tool-call slot.
    pub fn record_tool_error_at(
        &mut self,
        idx: usize,
        error: impl Into<String>,
    ) -> Result<(), ToolCallIndexError> {
        self.set_tool_outcome_at(idx, None, Some(error.into()))
    }
}

/// Record of a tool call made during a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnToolCall {
    /// Tool name.
    pub name: String,
    /// Parameters passed to the tool.
    pub parameters: serde_json::Value,
    /// Result from the tool (if successful).
    pub result: Option<serde_json::Value>,
    /// Error from the tool (if failed).
    pub error: Option<String>,
}
