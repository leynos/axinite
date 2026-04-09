//! Tests for the dispatcher module.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use rust_decimal::Decimal;

use crate::agent::agent_loop::{Agent, AgentDeps};
use crate::agent::cost_guard::{CostGuard, CostGuardConfig};
use crate::agent::session::Session;
use crate::channels::{ChannelManager, IncomingMessage};
use crate::tools::builtin::EchoTool;
use tokio::sync::Mutex;
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

use super::types::*;

/// Flexible mock LLM provider for unit tests.
pub(super) struct MockLlmProvider {
    name: &'static str,
    text: String,
    tool_responder:
        Arc<dyn Fn(ToolCompletionRequest) -> ToolCompletionResponse + Send + Sync>,
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
            tool_responder: Arc::new(|request| {
                if request.tools.is_empty() {
                    return ToolCompletionResponse {
                        content: Some("forced text response".to_string()),
                        tool_calls: Vec::new(),
                        input_tokens: 0,
                        output_tokens: 5,
                        finish_reason: FinishReason::Stop,
                        cache_read_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                    };
                }
                ToolCompletionResponse {
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
                }
            }),
        }
    }

    /// Always returns a call to a nonexistent tool.
    pub(super) fn failing_tool_call() -> Self {
        Self {
            name: "failing-tool-call",
            text: "forced text".to_string(),
            tool_responder: Arc::new(|request| {
                if request.tools.is_empty() {
                    return ToolCompletionResponse {
                        content: Some("forced text".to_string()),
                        tool_calls: Vec::new(),
                        input_tokens: 0,
                        output_tokens: 2,
                        finish_reason: FinishReason::Stop,
                        cache_read_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                    };
                }
                ToolCompletionResponse {
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
                }
            }),
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

/// Build a minimal `Agent` for unit testing (no DB, no workspace, no extensions).
pub(super) fn make_test_agent() -> Agent {
    let deps = AgentDeps {
        store: None,
        llm: Arc::new(MockLlmProvider::static_ok()),
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

/// Helper to build a test Agent with a custom LLM provider and
/// `max_tool_iterations` override.
pub(super) fn make_test_agent_with_llm(llm: Arc<dyn LlmProvider>, max_tool_iterations: usize) -> Agent {
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
