//! Tool execution logic for the chat delegate.
//!
//! Splits the 3-phase tool execution pipeline into cohesive submodules:
//! preflight, execution, postflight, and recording.

pub mod execution;
pub mod postflight;
pub mod preflight;
pub mod recording;

use uuid::Uuid;

use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::agent::session::PendingApproval;
use crate::channels::StatusUpdate;
use crate::error::Error;
use crate::llm::{ChatMessage, ReasoningContext};

fn build_pending_approval(
    delegate: &ChatDelegate<'_>,
    candidate: preflight::ApprovalCandidate,
    tool_calls: &[crate::llm::ToolCall],
    reason_ctx: &ReasoningContext,
) -> PendingApproval {
    let display_params = crate::tools::redact_params(
        &candidate.tool_call.arguments,
        candidate.tool.sensitive_params(),
    );
    PendingApproval {
        request_id: Uuid::new_v4(),
        tool_name: candidate.tool_call.name.clone(),
        parameters: candidate.tool_call.arguments.clone(),
        display_parameters: display_params,
        description: candidate.tool.description().to_string(),
        tool_call_id: candidate.tool_call.id.clone(),
        context_messages: reason_ctx.messages.clone(),
        deferred_tool_calls: tool_calls[candidate.idx + 1..].to_vec(),
        user_timezone: Some(delegate.user_tz.name().to_string()),
    }
}

/// Execute tool calls with 3-phase pipeline (preflight → execution → post-flight).
pub(crate) async fn execute_tool_calls(
    delegate: &ChatDelegate<'_>,
    tool_calls: Vec<crate::llm::ToolCall>,
    content: Option<String>,
    reason_ctx: &mut ReasoningContext,
) -> Result<Option<crate::agent::agentic_loop::LoopOutcome>, Error> {
    use crate::agent::agentic_loop::LoopOutcome;

    // Phase 1: run preflight (hooks, approval checks) FIRST so mutated
    // arguments are available before we commit anything to context or history.
    let (batch, approval_needed) = preflight::group_tool_calls(delegate, &tool_calls).await?;
    let preflight::ToolBatch {
        preflight,
        runnable,
    } = batch;

    let mut effective_tool_calls: Vec<crate::llm::ToolCall> =
        preflight.iter().map(|(tc, _)| tc.clone()).collect();
    if let Some(ref candidate) = approval_needed {
        effective_tool_calls.push(candidate.tool_call.clone());
    }

    reason_ctx
        .messages
        .push(ChatMessage::assistant_with_tool_calls(
            content,
            effective_tool_calls.clone(),
        ));

    let _ = delegate
        .agent
        .channels
        .send_status(
            &delegate.message.channel,
            StatusUpdate::Thinking(format!("Executing {} tool(s)...", tool_calls.len())),
            &delegate.message.metadata,
        )
        .await;

    recording::record_redacted_tool_calls(delegate, &effective_tool_calls).await;

    let mut exec_results = execution::run_phase2(delegate, preflight.len(), &runnable).await;
    let deferred_auth =
        postflight::run_postflight(delegate, preflight, &mut exec_results, reason_ctx).await;

    if let Some(instructions) = deferred_auth {
        return Ok(Some(LoopOutcome::Response(instructions)));
    }

    if let Some(candidate) = approval_needed {
        let pending = build_pending_approval(delegate, candidate, &tool_calls, reason_ctx);
        return Ok(Some(LoopOutcome::NeedApproval(Box::new(pending))));
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use rstest::rstest;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    use super::*;
    use crate::agent::agent_loop::{Agent, AgentDeps};
    use crate::agent::cost_guard::{CostGuard, CostGuardConfig};
    use crate::agent::session::Session;
    use crate::channels::{ChannelManager, IncomingMessage};
    use crate::config::{AgentConfig, SafetyConfig, SkillsConfig};
    use crate::context::{ContextManager, JobContext};
    use crate::hooks::{
        HookContext, HookEvent, HookFailureMode, HookOutcome, HookPoint, HookRegistry, NativeHook,
    };
    use crate::llm::LlmProvider;
    use crate::safety::SafetyLayer;
    use crate::testing::StubLlm;
    use crate::tools::{
        ApprovalRequirement, Tool, ToolError, ToolFuture, ToolOutput, ToolRegistry,
    };

    struct MutateToolCallHook;

    impl NativeHook for MutateToolCallHook {
        fn name(&self) -> &str {
            "mutate-tool-call"
        }

        fn hook_points(&self) -> &[HookPoint] {
            &[HookPoint::BeforeToolCall]
        }

        fn failure_mode(&self) -> HookFailureMode {
            HookFailureMode::FailClosed
        }

        async fn execute<'a>(
            &'a self,
            event: &'a HookEvent,
            _ctx: &'a HookContext,
        ) -> Result<HookOutcome, crate::hooks::HookError> {
            match event {
                HookEvent::ToolCall { parameters, .. } => {
                    let mut modified = parameters.clone();
                    modified["value"] = serde_json::json!("mutated");
                    Ok(HookOutcome::modify(modified.to_string()))
                }
                _ => Ok(HookOutcome::ok()),
            }
        }
    }

    struct ApprovalTool;

    impl Tool for ApprovalTool {
        fn name(&self) -> &str {
            "approval_tool"
        }

        fn description(&self) -> &str {
            "Approval-gated test tool"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": { "value": { "type": "string" } },
                "required": ["value"]
            })
        }

        fn execute<'a>(
            &'a self,
            _params: serde_json::Value,
            _ctx: &'a JobContext,
        ) -> ToolFuture<'a, Result<ToolOutput, ToolError>> {
            Box::pin(async move { Ok(ToolOutput::text("ok", Duration::from_secs(0))) })
        }

        fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
            ApprovalRequirement::Always
        }
    }

    async fn make_test_agent() -> Agent {
        let hooks = Arc::new(HookRegistry::new());
        hooks.register(Arc::new(MutateToolCallHook)).await;

        let tools = Arc::new(ToolRegistry::new());
        let registered = tools.register(Arc::new(ApprovalTool)).await;
        assert!(registered, "test tool registration should succeed");

        let deps = AgentDeps {
            store: None,
            llm: Arc::new(StubLlm::new("ok")) as Arc<dyn LlmProvider>,
            cheap_llm: None,
            safety: Arc::new(SafetyLayer::new(&SafetyConfig {
                max_output_length: 100_000,
                injection_check_enabled: false,
            })),
            tools,
            workspace: None,
            extension_manager: None,
            skill_registry: None::<Arc<std::sync::RwLock<crate::skills::SkillRegistry>>>,
            skill_catalog: None,
            skills_config: SkillsConfig::default(),
            hooks,
            cost_guard: Arc::new(CostGuard::new(CostGuardConfig::default())),
            sse_tx: None,
            http_interceptor: None,
            transcription: None,
            document_extraction: None,
        };

        Agent::new(
            AgentConfig {
                name: "test-agent".to_string(),
                max_parallel_jobs: 1,
                job_timeout: Duration::from_secs(60),
                stuck_threshold: Duration::from_secs(60),
                repair_check_interval: Duration::from_secs(30),
                max_repair_attempts: 1,
                use_planning: false,
                session_idle_timeout: Duration::from_secs(300),
                allow_local_tools: false,
                max_cost_per_day_cents: None,
                max_actions_per_hour: None,
                max_tool_iterations: 5,
                auto_approve_tools: false,
                default_timezone: "UTC".to_string(),
                max_tokens_per_job: 0,
            },
            deps,
            Arc::new(ChannelManager::new()),
            None,
            None,
            None,
            Some(Arc::new(ContextManager::new(1))),
            None,
        )
    }

    fn make_delegate<'a>(
        agent: &'a Agent,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        message: &'a IncomingMessage,
    ) -> ChatDelegate<'a> {
        ChatDelegate {
            agent,
            session,
            thread_id,
            message,
            job_ctx: JobContext::with_user(&message.user_id, &message.channel, "test session"),
            active_skills: vec![],
            cached_prompt: String::new(),
            cached_prompt_no_tools: String::new(),
            nudge_at: 0,
            force_text_at: 0,
            user_tz: chrono_tz::UTC,
        }
    }

    #[rstest]
    #[tokio::test]
    async fn execute_tool_calls_records_hook_mutated_arguments_in_reasoning_context() {
        let agent = make_test_agent().await;
        let message = IncomingMessage::new("web", "user-1", "run tool");
        let mut session = Session::new("user-1");
        let thread_id = {
            let thread = session.create_thread();
            thread.start_turn("run tool");
            thread.id
        };
        let session = Arc::new(Mutex::new(session));
        let delegate = make_delegate(&agent, session, thread_id, &message);
        let tool_call = crate::llm::ToolCall {
            id: "call-1".to_string(),
            name: "approval_tool".to_string(),
            arguments: serde_json::json!({ "value": "original" }),
        };
        let mut reason_ctx = ReasoningContext::default();

        let outcome = execute_tool_calls(
            &delegate,
            vec![tool_call],
            Some("thinking".to_string()),
            &mut reason_ctx,
        )
        .await
        .expect("tool execution should succeed");

        assert!(
            matches!(
                outcome,
                Some(crate::agent::agentic_loop::LoopOutcome::NeedApproval(_))
            ),
            "approval-gated test tool should stop at NeedApproval"
        );

        let last_message = reason_ctx
            .messages
            .last()
            .expect("assistant tool-call message should be recorded");
        let recorded_calls = last_message
            .tool_calls
            .as_ref()
            .expect("assistant message should include tool calls");
        assert_eq!(recorded_calls.len(), 1, "expected one recorded tool call");
        assert_eq!(
            recorded_calls[0].arguments["value"],
            serde_json::json!("mutated"),
            "reasoning context should record hook-mutated arguments"
        );
    }
}
