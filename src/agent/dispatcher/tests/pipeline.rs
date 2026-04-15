//! Integration-style tests for the dispatcher tool-execution pipeline.

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;

use crate::agent::session::PendingApproval;
use crate::channels::StatusUpdate;
use crate::context::JobContext;
use crate::llm::{ChatMessage, CompletionResponse, FinishReason, NativeLlmProvider, Role};
use crate::testing::StubChannel;
use crate::tools::{ApprovalRequirement, NativeTool, ToolError, ToolOutput};

use super::*;

struct TestPipelineTool {
    name: &'static str,
    description: &'static str,
    output_text: &'static str,
    approval_requirement: ApprovalRequirement,
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

struct PipelineProvider {
    name: &'static str,
    tool_calls: Vec<crate::llm::ToolCall>,
    final_text: &'static str,
    observed_tool_message_counts: Arc<StdMutex<Vec<usize>>>,
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
        self.observed_tool_message_counts
            .lock()
            .expect("tool message count lock poisoned")
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

async fn make_stubbed_channels(
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

async fn make_pipeline_agent(
    provider: Arc<dyn crate::llm::LlmProvider>,
    tools: Vec<Arc<dyn crate::tools::Tool>>,
    max_tool_iterations: usize,
    auto_approve_tools: bool,
) -> (Agent, Arc<std::sync::Mutex<Vec<StatusUpdate>>>) {
    let (channels, statuses) = make_stubbed_channels("test-chan").await;
    let deps = make_agent_deps(provider, false);
    deps.tools.register_builtin_tools();
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

    (agent, statuses)
}

async fn build_run_loop_ctx(
    prompt: &str,
) -> (
    Arc<Mutex<Session>>,
    uuid::Uuid,
    IncomingMessage,
    super::super::RunLoopCtx,
) {
    let session = Arc::new(Mutex::new(Session::new("test-user")));
    let thread_id = {
        let mut sess = session.lock().await;
        let thread = sess.create_thread();
        thread.start_turn(prompt);
        thread.id
    };
    let message = IncomingMessage::new("test-chan", "test-user", prompt);
    let ctx = super::super::RunLoopCtx {
        session: Arc::clone(&session),
        thread_id,
        initial_messages: vec![ChatMessage::user(prompt)],
    };

    (session, thread_id, message, ctx)
}

fn assert_thinking_status(statuses: &[StatusUpdate], expected: &str) {
    assert!(
        statuses
            .iter()
            .any(|status| matches!(status, StatusUpdate::Thinking(message) if message == expected)),
        "expected Thinking status `{expected}`, got: {statuses:?}"
    );
}

fn assert_tool_result_status(statuses: &[StatusUpdate], tool_name: &str) {
    assert!(
        statuses.iter().any(|status| matches!(
            status,
            StatusUpdate::ToolResult { name, preview }
                if name == tool_name && !preview.is_empty()
        )),
        "expected non-empty ToolResult preview for `{tool_name}`, got: {statuses:?}"
    );
}

fn assert_tool_completed_status(statuses: &[StatusUpdate], tool_name: &str) {
    assert!(
        statuses.iter().any(|status| matches!(
            status,
            StatusUpdate::ToolCompleted { name, success, .. }
                if name == tool_name && *success
        )),
        "expected successful ToolCompleted for `{tool_name}`, got: {statuses:?}"
    );
}

fn assert_tool_started_status(statuses: &[StatusUpdate], tool_name: &str) {
    assert!(
        statuses.iter().any(
            |status| matches!(status, StatusUpdate::ToolStarted { name } if name == tool_name)
        ),
        "expected ToolStarted for `{tool_name}`, got: {statuses:?}"
    );
}

#[tokio::test]
async fn pipeline_runs_inline_for_single_tool() {
    let observed_tool_message_counts = Arc::new(StdMutex::new(Vec::new()));
    let provider: Arc<dyn crate::llm::LlmProvider> = Arc::new(PipelineProvider {
        name: "pipeline-inline",
        tool_calls: vec![crate::llm::ToolCall {
            id: "call_echo".to_string(),
            name: "echo".to_string(),
            arguments: serde_json::json!({"message": "hello"}),
        }],
        final_text: "inline done",
        observed_tool_message_counts: Arc::clone(&observed_tool_message_counts),
    });
    let tools: Vec<Arc<dyn crate::tools::Tool>> = Vec::new();
    let (agent, statuses) = make_pipeline_agent(provider, tools, 6, false).await;
    let (_session, _thread_id, message, ctx) = build_run_loop_ctx("run echo").await;

    let result = agent
        .run_agentic_loop(&message, ctx)
        .await
        .expect("inline pipeline should succeed");

    match result {
        super::super::AgenticLoopResult::Response(text) => assert_eq!(text, "inline done"),
        super::super::AgenticLoopResult::NeedApproval { .. } => {
            panic!("single inline tool should not require approval");
        }
    }

    let captured = statuses.lock().expect("statuses lock poisoned");
    assert_tool_started_status(&captured, "echo");
    assert_tool_completed_status(&captured, "echo");
    assert_tool_result_status(&captured, "echo");

    let observed = observed_tool_message_counts
        .lock()
        .expect("tool message count lock poisoned")
        .clone();
    assert_eq!(
        observed,
        vec![0, 1],
        "provider should observe folded tool result"
    );
}

#[tokio::test]
async fn pipeline_runs_parallel_for_multiple_tools() {
    let observed_tool_message_counts = Arc::new(StdMutex::new(Vec::new()));
    let provider: Arc<dyn crate::llm::LlmProvider> = Arc::new(PipelineProvider {
        name: "pipeline-parallel",
        tool_calls: vec![
            crate::llm::ToolCall {
                id: "call_echo".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({"message": "hello"}),
            },
            crate::llm::ToolCall {
                id: "call_second".to_string(),
                name: "second_tool".to_string(),
                arguments: serde_json::json!({"message": "world"}),
            },
        ],
        final_text: "parallel done",
        observed_tool_message_counts: Arc::clone(&observed_tool_message_counts),
    });
    let tools: Vec<Arc<dyn crate::tools::Tool>> = vec![Arc::new(TestPipelineTool {
        name: "second_tool",
        description: "Second pipeline tool",
        output_text: "second result",
        approval_requirement: ApprovalRequirement::Never,
    })];
    let (agent, statuses) = make_pipeline_agent(provider, tools, 6, false).await;
    let (session, thread_id, message, ctx) = build_run_loop_ctx("run both tools").await;

    let result = agent
        .run_agentic_loop(&message, ctx)
        .await
        .expect("parallel pipeline should succeed");

    match result {
        super::super::AgenticLoopResult::Response(text) => assert_eq!(text, "parallel done"),
        super::super::AgenticLoopResult::NeedApproval { .. } => {
            panic!("parallel runnable tools should not require approval");
        }
    }

    {
        let captured = statuses.lock().expect("statuses lock poisoned");
        assert_thinking_status(&captured, "Executing 2 tool(s)...");
        assert_tool_completed_status(&captured, "echo");
        assert_tool_completed_status(&captured, "second_tool");
    }

    let observed = observed_tool_message_counts
        .lock()
        .expect("tool message count lock poisoned")
        .clone();
    assert_eq!(
        observed,
        vec![0, 2],
        "provider should observe two folded tool results"
    );

    let sess = session.lock().await;
    let thread = sess
        .threads
        .get(&thread_id)
        .expect("thread should still exist");
    let turn = thread.last_turn().expect("turn should exist");
    assert_eq!(
        turn.tool_calls.len(),
        2,
        "both tool calls should be recorded"
    );
}

#[tokio::test]
async fn pipeline_blocks_on_approval() {
    let provider: Arc<dyn crate::llm::LlmProvider> = Arc::new(PipelineProvider {
        name: "pipeline-approval",
        tool_calls: vec![
            crate::llm::ToolCall {
                id: "call_approval".to_string(),
                name: "approval_tool".to_string(),
                arguments: serde_json::json!({"message": "sensitive"}),
            },
            crate::llm::ToolCall {
                id: "call_deferred".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({"message": "later"}),
            },
        ],
        final_text: "should not reach text",
        observed_tool_message_counts: Arc::new(StdMutex::new(Vec::new())),
    });
    let tools: Vec<Arc<dyn crate::tools::Tool>> = vec![Arc::new(TestPipelineTool {
        name: "approval_tool",
        description: "Approval-gated tool",
        output_text: "approval result",
        approval_requirement: ApprovalRequirement::Always,
    })];
    let (agent, _statuses) = make_pipeline_agent(provider, tools, 6, false).await;
    let (_session, _thread_id, message, ctx) = build_run_loop_ctx("run approval tool").await;

    let result = agent
        .run_agentic_loop(&message, ctx)
        .await
        .expect("approval pipeline should return NeedApproval");

    match result {
        super::super::AgenticLoopResult::NeedApproval { pending } => {
            let PendingApproval {
                tool_call_id,
                deferred_tool_calls,
                ..
            } = pending;
            assert_eq!(tool_call_id, "call_approval");
            assert_eq!(deferred_tool_calls.len(), 1);
            assert_eq!(deferred_tool_calls[0].id, "call_deferred");
        }
        super::super::AgenticLoopResult::Response(text) => {
            panic!("expected NeedApproval, got response: {text}");
        }
    }
}
