//! Test doubles and arrangement helpers for the tool-execution pipeline
//! tests.
//!
//! Split from the parent module so each stays within the repository's
//! module-size limit.

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;

use crate::channels::StatusUpdate;
use crate::context::JobContext;
use crate::llm::{ChatMessage, CompletionResponse, FinishReason, NativeLlmProvider, Role};
use crate::testing::StubChannel;
use crate::tools::{ApprovalRequirement, NativeTool, ToolError, ToolOutput};

use super::*;

pub(super) struct TestPipelineTool {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
    pub(super) output_text: &'static str,
    pub(super) approval_requirement: ApprovalRequirement,
}

impl NativeTool for TestPipelineTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": { "type": "string" }
            }
        })
    }

    async fn execute(
        &self,
        _params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        Ok(ToolOutput::text(self.output_text, Instant::now().elapsed()))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        self.approval_requirement
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

pub(super) struct PipelineProvider {
    pub(super) name: &'static str,
    pub(super) tool_calls: Vec<crate::llm::ToolCall>,
    pub(super) final_text: &'static str,
    pub(super) observed_tool_message_counts: Arc<StdMutex<Vec<usize>>>,
}

impl NativeLlmProvider for PipelineProvider {
    fn model_name(&self) -> &str {
        self.name
    }

    fn cost_per_token(&self) -> (rust_decimal::Decimal, rust_decimal::Decimal) {
        (rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO)
    }

    async fn complete(
        &self,
        _request: crate::llm::CompletionRequest,
    ) -> Result<CompletionResponse, crate::error::LlmError> {
        Ok(CompletionResponse {
            content: self.final_text.to_string(),
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        request: crate::llm::ToolCompletionRequest,
    ) -> Result<crate::llm::ToolCompletionResponse, crate::error::LlmError> {
        let tool_message_count = request
            .messages
            .iter()
            .filter(|message| message.role == Role::Tool)
            .count();
        // The trait signature already returns `Result`, so a poisoned
        // observation lock is reported as a provider failure rather than
        // panicking inside the stub.
        self.observed_tool_message_counts
            .lock()
            .map_err(|_| crate::error::LlmError::RequestFailed {
                provider: self.name.to_string(),
                reason: "tool message count lock poisoned".to_string(),
            })?
            .push(tool_message_count);

        if tool_message_count >= self.tool_calls.len().max(1) {
            Ok(crate::llm::ToolCompletionResponse {
                content: Some(self.final_text.to_string()),
                tool_calls: Vec::new(),
                input_tokens: 0,
                output_tokens: 8,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        } else {
            Ok(crate::llm::ToolCompletionResponse {
                content: None,
                tool_calls: self.tool_calls.clone(),
                input_tokens: 0,
                output_tokens: 8,
                finish_reason: FinishReason::ToolUse,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }
    }
}

pub(super) async fn make_stubbed_channels(
    name: &str,
) -> (
    Arc<ChannelManager>,
    Arc<std::sync::Mutex<Vec<StatusUpdate>>>,
) {
    let (stub, _sender) = StubChannel::new(name);
    let statuses = stub.captured_statuses_handle();
    let channels = Arc::new(ChannelManager::new());
    channels.add(Box::new(stub)).await;
    (channels, statuses)
}

pub(super) async fn make_pipeline_agent(
    provider: Arc<dyn crate::llm::LlmProvider>,
    tools: Vec<Arc<dyn crate::tools::Tool>>,
    max_tool_iterations: usize,
    auto_approve_tools: bool,
) -> anyhow::Result<(Agent, Arc<std::sync::Mutex<Vec<StatusUpdate>>>)> {
    let (channels, statuses) = make_stubbed_channels("test-chan").await;
    let deps = make_agent_deps(provider, false);
    deps.tools.register_builtin_tools()?;
    for tool in tools {
        let _ = deps.tools.register(tool).await;
    }

    let agent = Agent::new(
        make_agent_config(max_tool_iterations, auto_approve_tools),
        deps,
        channels,
        None,
        None,
        None,
        Some(Arc::new(ContextManager::new(1))),
        None,
    );

    Ok((agent, statuses))
}

pub(super) async fn build_run_loop_ctx(
    prompt: &str,
) -> (
    Arc<Mutex<Session>>,
    uuid::Uuid,
    IncomingMessage,
    crate::agent::dispatcher::core::RunLoopCtx,
) {
    let session = Arc::new(Mutex::new(Session::new("test-user")));
    let thread_id = {
        let mut sess = session.lock().await;
        let thread = sess.create_thread();
        thread.start_turn(prompt);
        thread.id
    };
    let message = IncomingMessage::new("test-chan", "test-user", prompt);
    let ctx = crate::agent::dispatcher::core::RunLoopCtx {
        session: Arc::clone(&session),
        thread_id,
        initial_messages: vec![ChatMessage::user(prompt)],
    };

    (session, thread_id, message, ctx)
}

pub(super) fn assert_thinking_status(statuses: &[StatusUpdate], expected: &str) {
    assert!(
        statuses
            .iter()
            .any(|status| matches!(status, StatusUpdate::Thinking(message) if message == expected)),
        "expected Thinking status `{expected}`, got: {statuses:?}"
    );
}

pub(super) fn assert_tool_result_status(statuses: &[StatusUpdate], tool_name: &str) {
    assert!(
        statuses.iter().any(|status| matches!(
            status,
            StatusUpdate::ToolResult { name, preview }
                if name == tool_name && !preview.is_empty()
        )),
        "expected non-empty ToolResult preview for `{tool_name}`, got: {statuses:?}"
    );
}

pub(super) fn assert_tool_completed_status(statuses: &[StatusUpdate], tool_name: &str) {
    assert!(
        statuses.iter().any(|status| matches!(
            status,
            StatusUpdate::ToolCompleted { name, success, .. }
                if name == tool_name && *success
        )),
        "expected successful ToolCompleted for `{tool_name}`, got: {statuses:?}"
    );
}

pub(super) fn assert_tool_started_status(statuses: &[StatusUpdate], tool_name: &str) {
    assert!(
        statuses.iter().any(
            |status| matches!(status, StatusUpdate::ToolStarted { name } if name == tool_name)
        ),
        "expected ToolStarted for `{tool_name}`, got: {statuses:?}"
    );
}
