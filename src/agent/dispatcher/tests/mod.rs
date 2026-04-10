//! Tests for the dispatcher module.

use std::sync::{Arc, RwLock};
use std::time::Duration;

use rust_decimal::Decimal;

use crate::agent::agent_loop::{Agent, AgentDeps};
use crate::agent::cost_guard::{CostGuard, CostGuardConfig};
use crate::agent::session::Session;
use crate::channels::{ChannelManager, IncomingMessage};
use crate::config::{AgentConfig, SafetyConfig, SkillsConfig};
use crate::context::ContextManager;
use crate::error::Error;
use crate::hooks::HookRegistry;
use crate::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmProvider, Role, ToolCall,
    ToolCompletionRequest, ToolCompletionResponse,
};
use crate::safety::SafetyLayer;
use crate::skills::SkillRegistry;
use crate::tools::ToolRegistry;
use crate::tools::builtin::EchoTool;
use tokio::sync::Mutex;

use super::types::*;

/// Configuration for the forced-text arm of a mock tool responder.
struct MockTextResponse {
    text: &'static str,
    output_tokens: u32,
}

/// Configuration for the tool-call arm of a mock tool responder.
struct MockToolCall {
    name: &'static str,
    args: serde_json::Value,
    output_tokens: u32,
}

/// Build a `tool_responder` closure that returns forced text when the tool
/// list is empty and a single tool call otherwise.
fn text_or_tool_call_responder(
    text: MockTextResponse,
    call: MockToolCall,
) -> Arc<dyn Fn(ToolCompletionRequest) -> ToolCompletionResponse + Send + Sync> {
    Arc::new(move |request: ToolCompletionRequest| {
        if request.tools.is_empty() {
            ToolCompletionResponse {
                content: Some(text.text.to_string()),
                tool_calls: Vec::new(),
                input_tokens: 0,
                output_tokens: text.output_tokens,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }
        } else {
            ToolCompletionResponse {
                content: None,
                tool_calls: vec![ToolCall {
                    id: format!("call_{}", uuid::Uuid::new_v4()),
                    name: call.name.to_string(),
                    arguments: call.args.clone(),
                }],
                input_tokens: 0,
                output_tokens: call.output_tokens,
                finish_reason: FinishReason::ToolUse,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }
        }
    })
}

/// Flexible mock LLM provider for unit tests.
pub(super) struct MockLlmProvider {
    name: &'static str,
    text: String,
    tool_responder: Arc<dyn Fn(ToolCompletionRequest) -> ToolCompletionResponse + Send + Sync>,
}

impl MockLlmProvider {
    /// Returns a static text response with no tool calls.
    pub(super) fn static_ok() -> Self {
        Self {
            name: "static-mock",
            text: "ok".to_string(),
            tool_responder: Arc::new(|_| ToolCompletionResponse {
                content: Some("ok".to_string()),
                tool_calls: Vec::new(),
                input_tokens: 0,
                output_tokens: 0,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            }),
        }
    }

    /// Always returns a tool call to "echo" when tools are available.
    pub(super) fn always_tool_call() -> Self {
        Self {
            name: "always-tool-call",
            text: "forced text response".to_string(),
            tool_responder: text_or_tool_call_responder(
                MockTextResponse {
                    text: "forced text response",
                    output_tokens: 5,
                },
                MockToolCall {
                    name: "echo",
                    args: serde_json::json!({"message": "looping"}),
                    output_tokens: 5,
                },
            ),
        }
    }

    /// Always returns a call to a nonexistent tool.
    pub(super) fn failing_tool_call() -> Self {
        Self {
            name: "failing-tool-call",
            text: "forced text".to_string(),
            tool_responder: text_or_tool_call_responder(
                MockTextResponse {
                    text: "forced text",
                    output_tokens: 2,
                },
                MockToolCall {
                    name: "nonexistent_tool",
                    args: serde_json::json!({}),
                    output_tokens: 5,
                },
            ),
        }
    }
}

impl crate::llm::NativeLlmProvider for MockLlmProvider {
    fn model_name(&self) -> &str {
        self.name
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse, crate::error::LlmError> {
        Ok(CompletionResponse {
            content: self.text.clone(),
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, crate::error::LlmError> {
        Ok((self.tool_responder)(request))
    }
}

/// Construct `AgentDeps` for testing. Only `llm` and `injection_check_enabled` vary
/// between the two test-agent builders.
fn make_agent_deps(llm: Arc<dyn LlmProvider>, injection_check_enabled: bool) -> AgentDeps {
    AgentDeps {
        store: None,
        llm,
        cheap_llm: None,
        safety: Arc::new(SafetyLayer::new(&SafetyConfig {
            max_output_length: 100_000,
            injection_check_enabled,
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
    }
}

/// Construct `AgentConfig` for testing. Only `max_tool_iterations` and
/// `auto_approve_tools` vary between the two test-agent builders.
fn make_agent_config(max_tool_iterations: usize, auto_approve_tools: bool) -> AgentConfig {
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
        auto_approve_tools,
        default_timezone: "UTC".to_string(),
        max_tokens_per_job: 0,
    }
}

/// Build a minimal `Agent` for unit testing (no DB, no workspace, no extensions).
pub(super) fn make_test_agent() -> Agent {
    Agent::new(
        make_agent_config(50, false),
        make_agent_deps(Arc::new(MockLlmProvider::static_ok()), true),
        Arc::new(ChannelManager::new()),
        None,
        None,
        None,
        Some(Arc::new(ContextManager::new(1))),
        None,
    )
}

/// Helper to build a test Agent with a custom LLM provider and
/// `max_tool_iterations` override.
pub(super) fn make_test_agent_with_llm(
    llm: Arc<dyn LlmProvider>,
    max_tool_iterations: usize,
) -> Agent {
    Agent::new(
        make_agent_config(max_tool_iterations, true),
        make_agent_deps(llm, false),
        Arc::new(ChannelManager::new()),
        None,
        None,
        None,
        Some(Arc::new(ContextManager::new(1))),
        None,
    )
}

mod approval;
mod auth;
mod chat_tool;
mod compaction;
mod error_format;
mod image_sentinel;
mod loop_guard;
mod pending_approval;
mod preview;
mod skills;
