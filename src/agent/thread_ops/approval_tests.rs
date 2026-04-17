//! Unit tests for approval deferred-tool preflight behaviour.
//!
//! These tests keep the approval continuation path aligned with the chat
//! delegate by verifying that deferred tool calls honour the same
//! `BeforeToolCall` hook rewrites and rejections.

use std::sync::Arc;
use std::time::Duration;

use rstest::{fixture, rstest};
use tokio::sync::Mutex;

use super::*;
use crate::agent::cost_guard::{CostGuard, CostGuardConfig};
use crate::agent::{AgentDeps, SessionManager};
use crate::channels::{ChannelManager, IncomingMessage};
use crate::config::{AgentConfig, SafetyConfig, SkillsConfig};
use crate::context::JobContext;
use crate::hooks::{
    HookContext, HookError, HookEvent, HookFailureMode, HookOutcome, HookPoint, HookRegistry,
    NativeHook,
};
use crate::llm::LlmProvider;
use crate::safety::SafetyLayer;
use crate::testing::StubLlm;
use crate::tools::{ApprovalRequirement, Tool, ToolError, ToolFuture, ToolOutput, ToolRegistry};

struct DeferredTool;

impl Tool for DeferredTool {
    fn name(&self) -> &str {
        "deferred_tool"
    }

    fn description(&self) -> &str {
        "Deferred approval test tool"
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
        ApprovalRequirement::Never
    }
}

struct MutateDeferredHook;

impl NativeHook for MutateDeferredHook {
    fn name(&self) -> &str {
        "mutate-deferred-tool-call"
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
    ) -> Result<HookOutcome, HookError> {
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

struct RejectDeferredHook;

impl NativeHook for RejectDeferredHook {
    fn name(&self) -> &str {
        "reject-deferred-tool-call"
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
    ) -> Result<HookOutcome, HookError> {
        match event {
            HookEvent::ToolCall { .. } => Err(HookError::Rejected {
                reason: "blocked by test".to_string(),
            }),
            _ => Ok(HookOutcome::ok()),
        }
    }
}

#[fixture]
fn approval_message() -> IncomingMessage {
    IncomingMessage::new("web", "user-1", "approve")
}

async fn make_test_agent<H>(hook: Arc<H>) -> Agent
where
    H: NativeHook + 'static,
{
    let hooks = Arc::new(HookRegistry::new());
    hooks.register(hook).await;

    let tools = Arc::new(ToolRegistry::new());
    let registered = tools.register(Arc::new(DeferredTool)).await;
    assert!(registered, "deferred test tool registration should succeed");

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
        skill_registry: None,
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
        AgentConfig::for_testing(),
        deps,
        Arc::new(ChannelManager::new()),
        None,
        None,
        None,
        None,
        Some(Arc::new(SessionManager::new())),
    )
}

fn make_scope(message: &IncomingMessage) -> TurnScope {
    let mut session = Session::new(message.user_id.clone());
    let thread_id = session.create_thread().id;
    TurnScope::new(Arc::new(Mutex::new(session)), thread_id, message)
}

#[rstest]
#[tokio::test]
async fn preflight_deferred_tools_applies_hook_parameter_rewrites(
    approval_message: IncomingMessage,
) {
    let agent = make_test_agent(Arc::new(MutateDeferredHook)).await;
    let scope = make_scope(&approval_message);
    let deferred = vec![crate::llm::ToolCall {
        id: "call-1".to_string(),
        name: "deferred_tool".to_string(),
        arguments: serde_json::json!({ "value": "original" }),
    }];

    let (preflight, runnable, approval_needed) =
        agent.preflight_deferred_tools(&scope, &deferred).await;

    assert!(
        approval_needed.is_none(),
        "hook-mutated deferred tool should remain runnable"
    );
    assert_eq!(runnable.len(), 1, "expected one runnable deferred tool");
    assert_eq!(
        runnable[0].arguments["value"],
        serde_json::json!("mutated"),
        "deferred preflight should execute with hook-mutated arguments"
    );

    let (recorded_tc, recorded_outcome) = preflight
        .first()
        .expect("preflight should record the runnable deferred tool");
    assert_eq!(
        recorded_tc.arguments["value"],
        serde_json::json!("mutated"),
        "preflight record should keep the hook-mutated arguments"
    );
    assert!(
        matches!(recorded_outcome, DeferredPreflightOutcome::Runnable),
        "expected runnable preflight outcome after hook mutation"
    );
}

#[rstest]
#[tokio::test]
async fn preflight_deferred_tools_blocks_hook_rejections(approval_message: IncomingMessage) {
    let agent = make_test_agent(Arc::new(RejectDeferredHook)).await;
    let scope = make_scope(&approval_message);
    let deferred = vec![crate::llm::ToolCall {
        id: "call-1".to_string(),
        name: "deferred_tool".to_string(),
        arguments: serde_json::json!({ "value": "original" }),
    }];

    let (preflight, runnable, approval_needed) =
        agent.preflight_deferred_tools(&scope, &deferred).await;

    assert!(
        runnable.is_empty(),
        "rejected deferred tool should not enter the runnable batch"
    );
    assert!(
        approval_needed.is_none(),
        "hook rejection should stop before approval gating"
    );

    let (recorded_tc, recorded_outcome) = preflight
        .first()
        .expect("preflight should record the rejected deferred tool");
    assert_eq!(
        recorded_tc.arguments["value"],
        serde_json::json!("original"),
        "rejected tool should retain its original arguments"
    );
    match recorded_outcome {
        DeferredPreflightOutcome::Rejected(message) => assert!(
            message.contains("blocked by test"),
            "rejection should preserve the hook-provided reason"
        ),
        DeferredPreflightOutcome::Runnable => {
            panic!("expected rejected preflight outcome for hook-blocked tool")
        }
    }
}
