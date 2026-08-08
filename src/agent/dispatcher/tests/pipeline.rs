//! Integration-style tests for the dispatcher tool-execution pipeline.

use std::sync::{Arc, Mutex as StdMutex};

use crate::agent::session::PendingApproval;
use crate::tools::ApprovalRequirement;

use super::*;

mod support;

use support::*;

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
    let (agent, statuses) = make_pipeline_agent(provider, tools, 6, false)
        .await
        .expect("make_pipeline_agent should build");
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
    let (agent, statuses) = make_pipeline_agent(provider, tools, 6, false)
        .await
        .expect("make_pipeline_agent should build");
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
    let (agent, _statuses) = make_pipeline_agent(provider, tools, 6, false)
        .await
        .expect("make_pipeline_agent should build");
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
