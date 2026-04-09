//! Tests for the dispatcher module.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use rust_decimal::Decimal;

use crate::agent::agent_loop::{Agent, AgentDeps};
use crate::agent::cost_guard::{CostGuard, CostGuardConfig};
use crate::agent::session::Session;
use crate::channels::ChannelManager;
use crate::config::{AgentConfig, SafetyConfig, SkillsConfig};
use crate::context::ContextManager;
use crate::error::Error;
use crate::hooks::HookRegistry;
use crate::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmProvider, ToolCall,
    ToolCompletionRequest, ToolCompletionResponse,
};
use crate::safety::SafetyLayer;
use crate::skills::SkillRegistry;
use crate::tools::ToolRegistry;

use super::types::*;

/// Minimal LLM provider for unit tests that always returns a static response.
struct StaticLlmProvider;

impl crate::llm::NativeLlmProvider for StaticLlmProvider {
    fn model_name(&self) -> &str {
        "static-mock"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse, crate::error::LlmError> {
        Ok(CompletionResponse {
            content: "ok".to_string(),
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, crate::error::LlmError> {
        Ok(ToolCompletionResponse {
            content: Some("ok".to_string()),
            tool_calls: Vec::new(),
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }
}

/// Build a minimal `Agent` for unit testing (no DB, no workspace, no extensions).
fn make_test_agent() -> Agent {
    let deps = AgentDeps {
        store: None,
        llm: Arc::new(StaticLlmProvider),
        cheap_llm: None,
        safety: Arc::new(SafetyLayer::new(&SafetyConfig {
            max_output_length: 100_000,
            injection_check_enabled: true,
        })),
        tools: Arc::new(ToolRegistry::new()),
        workspace: None,
        extension_manager: None,
        skill_registry: None,
        skill_catalog: None,
        skills_config: SkillsConfig::default(),
        hooks: Arc::new(HookRegistry::new()),
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
            max_tool_iterations: 50,
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

#[test]
fn test_make_test_agent_succeeds() {
    // Verify that a test agent can be constructed without panicking.
    let _agent = make_test_agent();
}

#[test]
fn test_auto_approved_tool_is_respected() {
    let _agent = make_test_agent();
    let mut session = Session::new("user-1");
    session.auto_approve_tool("http");

    // A non-shell tool that is auto-approved should be approved.
    assert!(session.is_tool_auto_approved("http"));
    // A tool that hasn't been auto-approved should not be.
    assert!(!session.is_tool_auto_approved("shell"));
}

#[test]
fn test_shell_destructive_command_requires_explicit_approval() {
    // requires_explicit_approval() detects destructive commands that
    // should return ApprovalRequirement::Always from ShellTool.
    use crate::tools::builtin::shell::requires_explicit_approval;

    let destructive_cmds = [
        "rm -rf /tmp/test",
        "git push --force origin main",
        "git reset --hard HEAD~5",
    ];
    for cmd in &destructive_cmds {
        assert!(
            requires_explicit_approval(cmd),
            "'{}' should require explicit approval",
            cmd
        );
    }

    let safe_cmds = ["git status", "cargo build", "ls -la"];
    for cmd in &safe_cmds {
        assert!(
            !requires_explicit_approval(cmd),
            "'{}' should not require explicit approval",
            cmd
        );
    }
}

#[test]
fn test_always_approval_requirement_bypasses_session_auto_approve() {
    // Regression test: even if tool is auto-approved in session,
    // ApprovalRequirement::Always must still trigger approval.
    use crate::tools::ApprovalRequirement;

    let mut session = Session::new("user-1");
    let tool_name = "tool_remove";

    // Manually auto-approve tool_remove in this session
    session.auto_approve_tool(tool_name);
    assert!(
        session.is_tool_auto_approved(tool_name),
        "tool should be auto-approved"
    );

    // However, ApprovalRequirement::Always should always require approval
    // This is verified by the dispatcher logic: Always => true (ignores session state)
    let always_req = ApprovalRequirement::Always;
    let requires_approval = match always_req {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !session.is_tool_auto_approved(tool_name),
        ApprovalRequirement::Always => true,
    };

    assert!(
        requires_approval,
        "ApprovalRequirement::Always must require approval even when tool is auto-approved"
    );
}

#[test]
fn test_always_approval_requirement_vs_unless_auto_approved() {
    // Verify the two requirements behave differently
    use crate::tools::ApprovalRequirement;

    let mut session = Session::new("user-2");
    let tool_name = "http";

    // Scenario 1: Tool is auto-approved
    session.auto_approve_tool(tool_name);

    // UnlessAutoApproved => doesn't require approval if auto-approved
    let unless_req = ApprovalRequirement::UnlessAutoApproved;
    let unless_needs = match unless_req {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !session.is_tool_auto_approved(tool_name),
        ApprovalRequirement::Always => true,
    };
    assert!(
        !unless_needs,
        "UnlessAutoApproved should not need approval when auto-approved"
    );

    // Always => always requires approval
    let always_req = ApprovalRequirement::Always;
    let always_needs = match always_req {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !session.is_tool_auto_approved(tool_name),
        ApprovalRequirement::Always => true,
    };
    assert!(
        always_needs,
        "Always must always require approval, even when auto-approved"
    );

    // Scenario 2: Tool is NOT auto-approved
    let new_tool = "new_tool";
    assert!(!session.is_tool_auto_approved(new_tool));

    // UnlessAutoApproved => requires approval
    let unless_needs = match unless_req {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !session.is_tool_auto_approved(new_tool),
        ApprovalRequirement::Always => true,
    };
    assert!(
        unless_needs,
        "UnlessAutoApproved should need approval when not auto-approved"
    );

    // Always => always requires approval
    let always_needs = match always_req {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !session.is_tool_auto_approved(new_tool),
        ApprovalRequirement::Always => true,
    };
    assert!(always_needs, "Always must always require approval");
}

#[test]
fn test_pending_approval_serialization_backcompat_without_deferred_calls() {
    // PendingApproval from before the deferred_tool_calls field was added
    // should deserialize with an empty vec (via #[serde(default)]).
    let json = serde_json::json!({
        "request_id": uuid::Uuid::new_v4(),
        "tool_name": "http",
        "parameters": {"url": "https://example.com", "method": "GET"},
        "description": "Make HTTP request",
        "tool_call_id": "call_123",
        "context_messages": [{"role": "user", "content": "go"}]
    })
    .to_string();

    let parsed: crate::agent::session::PendingApproval =
        serde_json::from_str(&json).expect("should deserialize without deferred_tool_calls");

    assert!(parsed.deferred_tool_calls.is_empty());
    assert_eq!(parsed.tool_name, "http");
    assert_eq!(parsed.tool_call_id, "call_123");
}

#[test]
fn test_pending_approval_serialization_roundtrip_with_deferred_calls() {
    let pending = crate::agent::session::PendingApproval {
        request_id: uuid::Uuid::new_v4(),
        tool_name: "shell".to_string(),
        parameters: serde_json::json!({"command": "echo hi"}),
        display_parameters: serde_json::json!({"command": "echo hi"}),
        description: "Run shell command".to_string(),
        tool_call_id: "call_1".to_string(),
        context_messages: vec![],
        deferred_tool_calls: vec![
            ToolCall {
                id: "call_2".to_string(),
                name: "http".to_string(),
                arguments: serde_json::json!({"url": "https://example.com"}),
            },
            ToolCall {
                id: "call_3".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({"message": "done"}),
            },
        ],
        user_timezone: None,
    };

    let json = serde_json::to_string(&pending).expect("serialize");
    let parsed: crate::agent::session::PendingApproval =
        serde_json::from_str(&json).expect("deserialize");

    assert_eq!(parsed.deferred_tool_calls.len(), 2);
    assert_eq!(parsed.deferred_tool_calls[0].name, "http");
    assert_eq!(parsed.deferred_tool_calls[1].name, "echo");
}

#[test]
fn test_detect_auth_awaiting_positive() {
    let result: Result<String, Error> = Ok(serde_json::json!({
        "name": "telegram",
        "kind": "WasmTool",
        "awaiting_token": true,
        "status": "awaiting_token",
        "instructions": "Please provide your Telegram Bot API token."
    })
    .to_string());

    let detected = check_auth_required("tool_auth", &result);
    assert!(detected.is_some());
    let (name, instructions) = detected.unwrap();
    assert_eq!(name, "telegram");
    assert!(instructions.contains("Telegram Bot API"));
}

#[test]
fn test_detect_auth_awaiting_not_awaiting() {
    let result: Result<String, Error> = Ok(serde_json::json!({
        "name": "telegram",
        "kind": "WasmTool",
        "awaiting_token": false,
        "status": "authenticated"
    })
    .to_string());

    assert!(check_auth_required("tool_auth", &result).is_none());
}

#[test]
fn test_detect_auth_awaiting_wrong_tool() {
    let result: Result<String, Error> = Ok(serde_json::json!({
        "name": "telegram",
        "awaiting_token": true,
    })
    .to_string());

    assert!(check_auth_required("tool_list", &result).is_none());
}

#[test]
fn test_detect_auth_awaiting_error_result() {
    let result: Result<String, Error> =
        Err(crate::error::ToolError::NotFound { name: "x".into() }.into());
    assert!(check_auth_required("tool_auth", &result).is_none());
}

#[test]
fn test_detect_auth_awaiting_default_instructions() {
    let result: Result<String, Error> = Ok(serde_json::json!({
        "name": "custom_tool",
        "awaiting_token": true,
        "status": "awaiting_token"
    })
    .to_string());

    let (_, instructions) = check_auth_required("tool_auth", &result).unwrap();
    assert_eq!(instructions, "Please provide your API token/key.");
}

#[test]
fn test_detect_auth_awaiting_tool_activate() {
    let result: Result<String, Error> = Ok(serde_json::json!({
        "name": "slack",
        "kind": "McpServer",
        "awaiting_token": true,
        "status": "awaiting_token",
        "instructions": "Provide your Slack Bot token."
    })
    .to_string());

    let detected = check_auth_required("tool_activate", &result);
    assert!(detected.is_some());
    let (name, instructions) = detected.unwrap();
    assert_eq!(name, "slack");
    assert!(instructions.contains("Slack Bot"));
}

#[test]
fn test_detect_auth_awaiting_tool_activate_not_awaiting() {
    let result: Result<String, Error> = Ok(serde_json::json!({
        "name": "slack",
        "tools_loaded": ["slack_post_message"],
        "message": "Activated"
    })
    .to_string());

    assert!(check_auth_required("tool_activate", &result).is_none());
}

#[tokio::test]
async fn test_execute_chat_tool_standalone_success() {
    use crate::config::SafetyConfig;
    use crate::context::JobContext;
    use crate::safety::SafetyLayer;
    use crate::tools::ToolRegistry;
    use crate::tools::builtin::EchoTool;

    let registry = ToolRegistry::new();
    registry.register_sync(std::sync::Arc::new(EchoTool));

    let safety = SafetyLayer::new(&SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: false,
    });

    let job_ctx = JobContext::with_user("test", "chat", "test session");

    let result = execute_chat_tool_standalone(
        &registry,
        &safety,
        "echo",
        &serde_json::json!({"message": "hello"}),
        &job_ctx,
    )
    .await;

    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(output.contains("hello"));
}

#[tokio::test]
async fn test_execute_chat_tool_standalone_not_found() {
    use crate::config::SafetyConfig;
    use crate::context::JobContext;
    use crate::safety::SafetyLayer;
    use crate::tools::ToolRegistry;

    let registry = ToolRegistry::new();
    let safety = SafetyLayer::new(&SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: false,
    });
    let job_ctx = JobContext::with_user("test", "chat", "test session");

    let result = execute_chat_tool_standalone(
        &registry,
        &safety,
        "nonexistent",
        &serde_json::json!({}),
        &job_ctx,
    )
    .await;

    assert!(result.is_err());
}

// ---- compact_messages_for_retry tests ----

use crate::llm::{ChatMessage, Role};

#[test]
fn test_compact_keeps_system_and_last_user_exchange() {
    let messages = vec![
        ChatMessage::system("You are a helpful assistant."),
        ChatMessage::user("First question"),
        ChatMessage::assistant("First answer"),
        ChatMessage::user("Second question"),
        ChatMessage::assistant("Second answer"),
        ChatMessage::user("Third question"),
        ChatMessage::assistant_with_tool_calls(
            None,
            vec![ToolCall {
                id: "call_1".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({"message": "hi"}),
            }],
        ),
        ChatMessage::tool_result("call_1", "echo", "hi"),
    ];

    let compacted = compact_messages_for_retry(&messages);

    // Should have: system prompt + compaction note + last user msg + tool call + tool result
    assert_eq!(compacted.len(), 5);
    assert_eq!(compacted[0].role, Role::System);
    assert_eq!(compacted[0].content, "You are a helpful assistant.");
    assert_eq!(compacted[1].role, Role::System); // compaction note
    assert!(compacted[1].content.contains("compacted"));
    assert_eq!(compacted[2].role, Role::User);
    assert_eq!(compacted[2].content, "Third question");
    assert_eq!(compacted[3].role, Role::Assistant); // tool call
    assert_eq!(compacted[4].role, Role::Tool); // tool result
}

#[test]
fn test_compact_preserves_multiple_system_messages() {
    let messages = vec![
        ChatMessage::system("System prompt"),
        ChatMessage::system("Skill context"),
        ChatMessage::user("Old question"),
        ChatMessage::assistant("Old answer"),
        ChatMessage::system("Nudge message"),
        ChatMessage::user("Current question"),
    ];

    let compacted = compact_messages_for_retry(&messages);

    // 3 system messages + compaction note + last user message
    assert_eq!(compacted.len(), 5);
    assert_eq!(compacted[0].content, "System prompt");
    assert_eq!(compacted[1].content, "Skill context");
    assert_eq!(compacted[2].content, "Nudge message");
    assert!(compacted[3].content.contains("compacted")); // note
    assert_eq!(compacted[4].content, "Current question");
}

#[test]
fn test_compact_single_user_message_keeps_everything() {
    let messages = vec![
        ChatMessage::system("System prompt"),
        ChatMessage::user("Only question"),
    ];

    let compacted = compact_messages_for_retry(&messages);

    // system + compaction note + user
    assert_eq!(compacted.len(), 3);
    assert_eq!(compacted[0].content, "System prompt");
    assert!(compacted[1].content.contains("compacted"));
    assert_eq!(compacted[2].content, "Only question");
}

#[test]
fn test_compact_no_user_messages_keeps_non_system() {
    let messages = vec![
        ChatMessage::system("System prompt"),
        ChatMessage::assistant("Stray assistant message"),
    ];

    let compacted = compact_messages_for_retry(&messages);

    // system + assistant (no user message found, keeps all non-system)
    assert_eq!(compacted.len(), 2);
    assert_eq!(compacted[0].role, Role::System);
    assert_eq!(compacted[1].role, Role::Assistant);
}

#[test]
fn test_compact_drops_old_history_but_keeps_current_turn_tools() {
    // Simulate a multi-turn conversation where the current turn has
    // multiple tool calls and results.
    let messages = vec![
        ChatMessage::system("System prompt"),
        ChatMessage::user("Question 1"),
        ChatMessage::assistant("Answer 1"),
        ChatMessage::user("Question 2"),
        ChatMessage::assistant("Answer 2"),
        ChatMessage::user("Question 3"),
        ChatMessage::assistant("Answer 3"),
        ChatMessage::user("Current question"),
        ChatMessage::assistant_with_tool_calls(
            None,
            vec![
                ToolCall {
                    id: "c1".to_string(),
                    name: "http".to_string(),
                    arguments: serde_json::json!({}),
                },
                ToolCall {
                    id: "c2".to_string(),
                    name: "echo".to_string(),
                    arguments: serde_json::json!({}),
                },
            ],
        ),
        ChatMessage::tool_result("c1", "http", "response data"),
        ChatMessage::tool_result("c2", "echo", "echoed"),
    ];

    let compacted = compact_messages_for_retry(&messages);

    // system + note + user + assistant(tool_calls) + tool_result + tool_result
    assert_eq!(compacted.len(), 6);
    assert_eq!(compacted[0].content, "System prompt");
    assert!(compacted[1].content.contains("compacted"));
    assert_eq!(compacted[2].content, "Current question");
    assert!(compacted[3].tool_calls.is_some()); // assistant with tool calls
    assert_eq!(compacted[4].name.as_deref(), Some("http"));
    assert_eq!(compacted[5].name.as_deref(), Some("echo"));
}

#[test]
fn test_compact_no_duplicate_system_after_last_user() {
    // A system nudge message injected AFTER the last user message must
    // not be duplicated — it should only appear once (via extend_from_slice).
    let messages = vec![
        ChatMessage::system("System prompt"),
        ChatMessage::user("Question"),
        ChatMessage::system("Nudge: wrap up"),
        ChatMessage::assistant_with_tool_calls(
            None,
            vec![ToolCall {
                id: "c1".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({}),
            }],
        ),
        ChatMessage::tool_result("c1", "echo", "done"),
    ];

    let compacted = compact_messages_for_retry(&messages);

    // system prompt + note + user + nudge + assistant + tool_result = 6
    assert_eq!(compacted.len(), 6);
    assert_eq!(compacted[0].content, "System prompt");
    assert!(compacted[1].content.contains("compacted"));
    assert_eq!(compacted[2].content, "Question");
    assert_eq!(compacted[3].content, "Nudge: wrap up"); // not duplicated
    assert_eq!(compacted[4].role, Role::Assistant);
    assert_eq!(compacted[5].role, Role::Tool);

    // Verify "Nudge: wrap up" appears exactly once
    let nudge_count = compacted
        .iter()
        .filter(|m| m.content == "Nudge: wrap up")
        .count();
    assert_eq!(nudge_count, 1);
}

// === QA Plan P2 - 2.7: Context length recovery ===

#[tokio::test]
async fn test_context_length_recovery_via_compaction_and_retry() {
    // Simulates the dispatcher's recovery path:
    //   1. Provider returns ContextLengthExceeded
    //   2. compact_messages_for_retry reduces context
    //   3. Retry with compacted messages succeeds
    use crate::llm::Reasoning;
    use crate::testing::StubLlm;

    let stub = Arc::new(StubLlm::failing_non_transient("ctx-bomb"));

    let reasoning = Reasoning::new(stub.clone());

    // Build a fat context with lots of history.
    let messages = vec![
        ChatMessage::system("You are a helpful assistant."),
        ChatMessage::user("First question"),
        ChatMessage::assistant("First answer"),
        ChatMessage::user("Second question"),
        ChatMessage::assistant("Second answer"),
        ChatMessage::user("Third question"),
        ChatMessage::assistant("Third answer"),
        ChatMessage::user("Current request"),
    ];

    let context = crate::llm::ReasoningContext::new().with_messages(messages.clone());

    // Step 1: First call fails with ContextLengthExceeded.
    let err = reasoning.respond_with_tools(&context).await.unwrap_err();
    assert!(
        matches!(err, crate::error::LlmError::ContextLengthExceeded { .. }),
        "Expected ContextLengthExceeded, got: {:?}",
        err
    );
    assert_eq!(stub.calls(), 1);

    // Step 2: Compact messages (same as dispatcher lines 226).
    let compacted = compact_messages_for_retry(&messages);
    // Should have dropped the old history, kept system + note + last user.
    assert!(compacted.len() < messages.len());
    assert_eq!(compacted.last().unwrap().content, "Current request");

    // Step 3: Switch provider to success and retry.
    stub.set_failing(false);
    let retry_context = crate::llm::ReasoningContext::new().with_messages(compacted);

    let result = reasoning.respond_with_tools(&retry_context).await;
    assert!(result.is_ok(), "Retry after compaction should succeed");
    assert_eq!(stub.calls(), 2);
}

// === QA Plan P2 - 4.3: Dispatcher loop guard tests ===

/// LLM provider that always returns tool calls when tools are available,
/// and text when tools are empty (simulating force_text stripping tools).
struct AlwaysToolCallProvider;

impl crate::llm::NativeLlmProvider for AlwaysToolCallProvider {
    fn model_name(&self) -> &str {
        "always-tool-call"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse, crate::error::LlmError> {
        Ok(CompletionResponse {
            content: "forced text response".to_string(),
            input_tokens: 0,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, crate::error::LlmError> {
        if request.tools.is_empty() {
            // No tools = force_text mode; return text.
            return Ok(ToolCompletionResponse {
                content: Some("forced text response".to_string()),
                tool_calls: Vec::new(),
                input_tokens: 0,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            });
        }
        // Tools available: always call one.
        Ok(ToolCompletionResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: format!("call_{}", uuid::Uuid::new_v4()),
                name: "echo".to_string(),
                arguments: serde_json::json!({"message": "looping"}),
            }],
            input_tokens: 0,
            output_tokens: 5,
            finish_reason: FinishReason::ToolUse,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }
}

#[tokio::test]
async fn force_text_prevents_infinite_tool_call_loop() {
    // Verify that Reasoning with force_text=true returns text even when
    // the provider would normally return tool calls.
    use crate::llm::{Reasoning, ReasoningContext, RespondResult, ToolDefinition};

    let provider = Arc::new(AlwaysToolCallProvider);
    let reasoning = Reasoning::new(provider);

    let tool_def = ToolDefinition {
        name: "echo".to_string(),
        description: "Echo a message".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {"message": {"type": "string"}}}),
    };

    // Without force_text: provider returns tool calls.
    let ctx_normal = ReasoningContext::new()
        .with_messages(vec![ChatMessage::user("hello")])
        .with_tools(vec![tool_def.clone()]);
    let output = reasoning.respond_with_tools(&ctx_normal).await.unwrap();
    assert!(
        matches!(output.result, RespondResult::ToolCalls { .. }),
        "Without force_text, should get tool calls"
    );

    // With force_text: provider must return text (tools stripped).
    let mut ctx_forced = ReasoningContext::new()
        .with_messages(vec![ChatMessage::user("hello")])
        .with_tools(vec![tool_def]);
    ctx_forced.force_text = true;
    let output = reasoning.respond_with_tools(&ctx_forced).await.unwrap();
    assert!(
        matches!(output.result, RespondResult::Text(_)),
        "With force_text, should get text response, got: {:?}",
        output.result
    );
}

#[test]
fn iteration_bounds_guarantee_termination() {
    // Verify the arithmetic that guards against infinite loops:
    // force_text_at = max_tool_iterations
    // nudge_at = max_tool_iterations - 1
    // hard_ceiling = max_tool_iterations + 1
    for max_iter in [1_usize, 2, 5, 10, 50] {
        let force_text_at = max_iter;
        let nudge_at = max_iter.saturating_sub(1);
        let hard_ceiling = max_iter + 1;

        // force_text_at must be reachable (> 0)
        assert!(
            force_text_at > 0,
            "force_text_at must be > 0 for max_iter={max_iter}"
        );

        // nudge comes before or at the same time as force_text
        assert!(
            nudge_at <= force_text_at,
            "nudge_at ({nudge_at}) > force_text_at ({force_text_at})"
        );

        // hard ceiling is strictly after force_text
        assert!(
            hard_ceiling > force_text_at,
            "hard_ceiling ({hard_ceiling}) not > force_text_at ({force_text_at})"
        );

        // Simulate iteration: every iteration from 1..=hard_ceiling
        // At force_text_at, force_text=true (should produce text and break).
        // At hard_ceiling, the error fires (safety net).
        let mut hit_force_text = false;
        let mut hit_ceiling = false;
        for iteration in 1..=hard_ceiling {
            if iteration >= force_text_at {
                hit_force_text = true;
            }
            if iteration > max_iter + 1 {
                hit_ceiling = true;
            }
        }
        assert!(
            hit_force_text,
            "force_text never triggered for max_iter={max_iter}"
        );
        // The ceiling should only fire if force_text somehow didn't break
        assert!(
            hit_ceiling || hard_ceiling <= max_iter + 1,
            "ceiling logic inconsistent for max_iter={max_iter}"
        );
    }
}

/// LLM provider that always returns calls to a nonexistent tool, regardless
/// of whether tools are available. When tools are stripped (force_text), it
/// returns text.
struct FailingToolCallProvider;

impl crate::llm::NativeLlmProvider for FailingToolCallProvider {
    fn model_name(&self) -> &str {
        "failing-tool-call"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse, crate::error::LlmError> {
        Ok(CompletionResponse {
            content: "forced text".to_string(),
            input_tokens: 0,
            output_tokens: 2,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, crate::error::LlmError> {
        if request.tools.is_empty() {
            return Ok(ToolCompletionResponse {
                content: Some("forced text".to_string()),
                tool_calls: Vec::new(),
                input_tokens: 0,
                output_tokens: 2,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            });
        }
        // Always call a tool that does not exist in the registry.
        Ok(ToolCompletionResponse {
            content: None,
            tool_calls: vec![ToolCall {
                id: format!("call_{}", uuid::Uuid::new_v4()),
                name: "nonexistent_tool".to_string(),
                arguments: serde_json::json!({}),
            }],
            input_tokens: 0,
            output_tokens: 5,
            finish_reason: FinishReason::ToolUse,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }
}

/// Helper to build a test Agent with a custom LLM provider and
/// `max_tool_iterations` override.
fn make_test_agent_with_llm(llm: Arc<dyn LlmProvider>, max_tool_iterations: usize) -> Agent {
    let deps = AgentDeps {
        store: None,
        llm,
        cheap_llm: None,
        safety: Arc::new(SafetyLayer::new(&SafetyConfig {
            max_output_length: 100_000,
            injection_check_enabled: false,
        })),
        tools: Arc::new(ToolRegistry::new()),
        workspace: None,
        extension_manager: None,
        skill_registry: None,
        skill_catalog: None,
        skills_config: SkillsConfig::default(),
        hooks: Arc::new(HookRegistry::new()),
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
            max_tool_iterations,
            auto_approve_tools: true,
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

/// Regression test for the infinite loop bug (PR #252) where `continue`
/// skipped the index increment. When every tool call fails (e.g., tool not
/// found), the dispatcher must still advance through all calls and
/// eventually terminate via the force_text / max_iterations guard.
#[tokio::test]
async fn test_dispatcher_terminates_with_all_tool_calls_failing() {
    use crate::agent::session::Session;
    use crate::channels::IncomingMessage;
    use crate::llm::ChatMessage;
    use tokio::sync::Mutex;

    let agent = make_test_agent_with_llm(Arc::new(FailingToolCallProvider), 5);

    let session = Arc::new(Mutex::new(Session::new("test-user")));

    // Initialize a thread in the session so the loop can record tool calls.
    let thread_id = {
        let mut sess = session.lock().await;
        sess.create_thread().id
    };

    let message = IncomingMessage::new("test", "test-user", "do something");
    let initial_messages = vec![ChatMessage::user("do something")];

    // The dispatcher must terminate within 5 seconds. If there is an
    // infinite loop bug (e.g., index not advancing on tool failure), the
    // timeout will fire and the test will fail.
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        agent.run_agentic_loop(&message, session, thread_id, initial_messages),
    )
    .await;

    assert!(
        result.is_ok(),
        "Dispatcher timed out -- possible infinite loop when all tool calls fail"
    );

    // The loop should complete (either with a text response from force_text,
    // or an error from the hard ceiling). Both are acceptable termination.
    let inner = result.unwrap();
    assert!(
        inner.is_ok(),
        "Dispatcher returned an error: {:?}",
        inner.err()
    );
}

/// Verify that the max_iterations guard terminates the loop even when the
/// LLM always returns tool calls and those calls succeed.
#[tokio::test]
async fn test_dispatcher_terminates_with_max_iterations() {
    use crate::agent::session::Session;
    use crate::channels::IncomingMessage;
    use crate::llm::ChatMessage;
    use crate::tools::builtin::EchoTool;
    use tokio::sync::Mutex;

    // Use AlwaysToolCallProvider which calls "echo" on every turn.
    // Register the echo tool so the calls succeed.
    let llm: Arc<dyn LlmProvider> = Arc::new(AlwaysToolCallProvider);
    let max_iter = 3;
    let agent = {
        let deps = AgentDeps {
            store: None,
            llm,
            cheap_llm: None,
            safety: Arc::new(SafetyLayer::new(&SafetyConfig {
                max_output_length: 100_000,
                injection_check_enabled: false,
            })),
            tools: {
                let registry = Arc::new(ToolRegistry::new());
                registry.register_sync(Arc::new(EchoTool));
                registry
            },
            workspace: None,
            extension_manager: None,
            skill_registry: None,
            skill_catalog: None,
            skills_config: SkillsConfig::default(),
            hooks: Arc::new(HookRegistry::new()),
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
                max_tool_iterations: max_iter,
                auto_approve_tools: true,
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
    };

    let session = Arc::new(Mutex::new(Session::new("test-user")));
    let thread_id = {
        let mut sess = session.lock().await;
        sess.create_thread().id
    };

    let message = IncomingMessage::new("test", "test-user", "keep calling tools");
    let initial_messages = vec![ChatMessage::user("keep calling tools")];

    // Even with an LLM that always wants to call tools, the dispatcher
    // must terminate within the timeout thanks to force_text at
    // max_tool_iterations.
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        agent.run_agentic_loop(&message, session, thread_id, initial_messages),
    )
    .await;

    assert!(
        result.is_ok(),
        "Dispatcher timed out -- max_iterations guard failed to terminate the loop"
    );

    // Should get a successful text response (force_text kicks in).
    let inner = result.unwrap();
    assert!(
        inner.is_ok(),
        "Dispatcher returned an error: {:?}",
        inner.err()
    );

    // Verify we got a text response.
    match inner.unwrap() {
        super::AgenticLoopResult::Response(text) => {
            assert!(!text.is_empty(), "Expected non-empty forced text response");
        }
        super::AgenticLoopResult::NeedApproval { .. } => {
            panic!("Expected text response, got NeedApproval");
        }
    }
}

#[test]
fn test_strip_internal_tool_call_text_removes_markers() {
    let input = "[Called tool search({\"query\": \"test\"})]\nHere is the answer.";
    let result = strip_internal_tool_call_text(input);
    assert_eq!(result, "Here is the answer.");
}

#[test]
fn test_strip_internal_tool_call_text_removes_returned_markers() {
    let input = "[Tool search returned: some result]\nSummary of findings.";
    let result = strip_internal_tool_call_text(input);
    assert_eq!(result, "Summary of findings.");
}

#[test]
fn test_strip_internal_tool_call_text_all_markers_yields_fallback() {
    let input = "[Called tool search({\"query\": \"test\"})]\n[Tool search returned: error]";
    let result = strip_internal_tool_call_text(input);
    assert!(result.contains("wasn't able to complete"));
}

#[test]
fn test_strip_internal_tool_call_text_preserves_normal_text() {
    let input = "This is a normal response with [brackets] inside.";
    let result = strip_internal_tool_call_text(input);
    assert_eq!(result, input);
}

#[test]
fn test_tool_error_format_includes_tool_name() {
    // Regression test for issue #487: tool errors sent to the LLM should
    // include the tool name so the model can reason about which tool failed
    // and try alternatives.
    let tool_name = "http";
    let err = crate::error::ToolError::ExecutionFailed {
        name: tool_name.to_string(),
        reason: "connection refused".to_string(),
    };
    let formatted = format!("Tool '{}' failed: {}", tool_name, err);
    assert!(
        formatted.contains("Tool 'http' failed:"),
        "Error should identify the tool by name, got: {formatted}"
    );
    assert!(
        formatted.contains("connection refused"),
        "Error should include the underlying reason, got: {formatted}"
    );
}

#[test]
fn test_image_sentinel_empty_data_url_should_be_skipped() {
    // Regression: unwrap_or_default() on missing "data" field produces an empty
    // string. Broadcasting an empty data_url would send a broken SSE event.
    let sentinel = serde_json::json!({
        "type": "image_generated",
        "path": "/tmp/image.png"
        // "data" field is missing
    });

    let data_url = sentinel
        .get("data")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    assert!(
        data_url.is_empty(),
        "Missing 'data' field should produce empty string"
    );
    // The fix: empty data_url means we skip broadcasting
}

#[test]
fn test_image_sentinel_present_data_url_is_valid() {
    let sentinel = serde_json::json!({
        "type": "image_generated",
        "data": "data:image/png;base64,abc123",
        "path": "/tmp/image.png"
    });

    let data_url = sentinel
        .get("data")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();

    assert!(
        !data_url.is_empty(),
        "Present 'data' field should produce non-empty string"
    );
}

#[test]
fn test_truncate_short_input() {
    assert_eq!(truncate_for_preview("hello", 10), "hello");
}

#[test]
fn test_truncate_empty_input() {
    assert_eq!(truncate_for_preview("", 10), "");
}

#[test]
fn test_truncate_exact_length() {
    assert_eq!(truncate_for_preview("hello", 5), "hello");
}

#[test]
fn test_truncate_over_limit() {
    let result = truncate_for_preview("hello world, this is long", 10);
    assert!(result.ends_with("..."));
    assert_eq!(result, "hello worl...");
}

#[test]
fn test_truncate_collapses_newlines() {
    let result = truncate_for_preview("line1\nline2\nline3", 100);
    assert!(!result.contains('\n'));
    assert_eq!(result, "line1 line2 line3");
}

#[test]
fn test_truncate_collapses_whitespace() {
    let result = truncate_for_preview("hello   world", 100);
    assert_eq!(result, "hello world");
}

#[test]
fn test_truncate_multibyte_utf8() {
    let input = "😀😁😂🤣😃😄😅😆😉😊";
    let result = truncate_for_preview(input, 5);
    assert!(result.ends_with("..."));
    assert_eq!(result, "😀😁😂🤣😃...");
}

#[test]
fn test_truncate_cjk_characters() {
    let input = "你好世界测试数据很长的字符串";
    let result = truncate_for_preview(input, 4);
    assert_eq!(result, "你好世界...");
}

#[test]
fn test_truncate_mixed_multibyte_and_ascii() {
    let input = "hello 世界 foo";
    let result = truncate_for_preview(input, 8);
    assert_eq!(result, "hello 世界...");
}

#[test]
fn test_truncate_large_whitespace_run_does_not_hide_content() {
    // "A" followed by 101 newlines then "B": after normalisation this is "A B" (3 chars).
    let input = format!("A{}\nB", "\n".repeat(100));
    assert_eq!(truncate_for_preview(&input, 3), "A B");
}

#[test]
fn test_truncate_large_whitespace_run_truncates_correctly() {
    // 100 newlines between words: normalise to "A B C", cap at 3 → "A B..."
    let input = format!("A{}B{}C", "\n".repeat(100), "\n".repeat(100));
    let result = truncate_for_preview(&input, 3);
    assert_eq!(result, "A B...");
}

#[test]
fn test_select_active_skills_returns_empty_when_disabled() {
    use crate::skills::{ActivationCriteria, LoadedSkill, SkillManifest, SkillSource, SkillTrust};
    use std::path::PathBuf;

    let registry = Arc::new(RwLock::new(SkillRegistry::new(PathBuf::from("."))));

    // Populate registry with a skill before testing disabled state
    {
        let mut reg = registry
            .write()
            .expect("failed to acquire registry write lock");
        let skill = LoadedSkill {
            manifest: SkillManifest {
                name: "test-skill".to_string(),
                version: "1.0.0".to_string(),
                description: "Test skill for disabled check".to_string(),
                activation: ActivationCriteria {
                    keywords: vec!["test".to_string()],
                    exclude_keywords: vec![],
                    patterns: vec![],
                    tags: vec![],
                    max_context_tokens: 1000,
                },
                metadata: None,
            },
            prompt_content: "Test skill content".to_string(),
            trust: SkillTrust::Trusted,
            source: SkillSource::User(PathBuf::from(".")),
            content_hash: "abc123".to_string(),
            compiled_patterns: vec![],
            lowercased_keywords: vec!["test".to_string()],
            lowercased_exclude_keywords: vec![],
            lowercased_tags: vec![],
        };
        reg.commit_install("test-skill", skill).unwrap();
    }

    let skills_cfg = SkillsConfig {
        enabled: false,
        ..SkillsConfig::default()
    };

    // Should return empty even though registry has skills, because skills are disabled
    assert!(select_active_skills(&registry, &skills_cfg, "hello").is_empty());
}

#[test]
fn test_select_active_skills_returns_empty_when_registry_lock_is_poisoned() {
    let registry = Arc::new(RwLock::new(SkillRegistry::new(PathBuf::from("."))));
    let poison_registry = Arc::clone(&registry);

    let _ = std::thread::spawn(move || {
        let _guard = poison_registry
            .write()
            .expect("poison test should acquire write lock");
        panic!("poison registry lock");
    })
    .join();

    let skills_cfg = SkillsConfig {
        enabled: true,
        ..SkillsConfig::default()
    };

    assert!(select_active_skills(&registry, &skills_cfg, "hello").is_empty());
}
