//! Builder utilities for assembling a realistic [`TestRig`].
//!
//! `TestRigBuilder` wires [`TestRig`], [`TestChannelHandle`],
//! [`InstrumentedLlm`], [`TestChannel`], [`TraceLlm`], and optional
//! `ReplayingHttpInterceptor` support into a fully running agent loop.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use ironclaw::agent::{Agent, AgentDeps};
use ironclaw::app::{AppBuilder, AppBuilderFlags, AppComponents};
use ironclaw::channels::web::log_layer::LogBroadcaster;
use ironclaw::config::Config;
use ironclaw::db::Database;
use ironclaw::llm::recording::{HttpExchange, ReplayingHttpInterceptor, TraceResponse, TraceStep};
use ironclaw::llm::{LlmProvider, SessionConfig, SessionManager};
use ironclaw::tools::Tool;

use crate::support::instrumented_llm::InstrumentedLlm;
use crate::support::test_channel::TestChannel;
use crate::support::trace_provider::TraceLlm;
use crate::support::trace_types::LlmTrace;

use super::{TestChannelHandle, TestRig};

mod assembly;

/// Builder for constructing a `TestRig`.
pub struct TestRigBuilder {
    trace: Option<LlmTrace>,
    llm: Option<Arc<dyn LlmProvider>>,
    max_tool_iterations: usize,
    injection_check: bool,
    auto_approve_tools: Option<bool>,
    enable_skills: bool,
    enable_routines: bool,
    http_exchanges: Vec<HttpExchange>,
    extra_tools: Vec<Arc<dyn Tool>>,
}

fn register_job_tools_for_tests(
    components: &ironclaw::app::AppComponents,
    scheduler_slot: &ironclaw::tools::builtin::SchedulerSlot,
) {
    components
        .tools
        .register_job_tools(ironclaw::tools::RegisterJobToolsOptions {
            context_manager: Arc::clone(&components.context_manager),
            scheduler_slot: Some(scheduler_slot.clone()),
            job_manager: None,
            store: components.db.clone(),
            job_event_tx: None,
            inject_tx: None,
            prompt_queue: None,
            secrets_store: None,
        });
}

impl TestRigBuilder {
    /// Create a new builder with defaults.
    pub fn new() -> Self {
        Self {
            trace: None,
            llm: None,
            max_tool_iterations: 10,
            injection_check: false,
            auto_approve_tools: None,
            enable_skills: false,
            enable_routines: false,
            http_exchanges: Vec::new(),
            extra_tools: Vec::new(),
        }
    }

    /// Set the LLM trace to replay.
    pub fn with_trace(mut self, trace: LlmTrace) -> Self {
        self.trace = Some(trace);
        self
    }

    /// Override the LLM provider directly (takes precedence over trace).
    pub fn with_llm(mut self, llm: Arc<dyn LlmProvider>) -> Self {
        self.llm = Some(llm);
        self
    }

    /// Set the maximum number of tool iterations per agentic loop invocation.
    pub fn with_max_tool_iterations(mut self, n: usize) -> Self {
        self.max_tool_iterations = n;
        self
    }

    /// Register additional custom tools (for example stub tools for testing).
    pub fn with_extra_tools(mut self, tools: Vec<Arc<dyn Tool>>) -> Self {
        self.extra_tools = tools;
        self
    }

    /// Enable prompt injection detection in the safety layer.
    pub fn with_injection_check(mut self, enable: bool) -> Self {
        self.injection_check = enable;
        self
    }

    /// Override agent-level automatic approval of `UnlessAutoApproved` tools.
    pub fn with_auto_approve_tools(mut self, enable: bool) -> Self {
        self.auto_approve_tools = Some(enable);
        self
    }

    /// Enable skill discovery and registration for this test rig.
    pub fn with_skills(mut self) -> Self {
        self.enable_skills = true;
        self
    }

    /// Enable the routines system so the scheduler is wired with a `RoutineEngine`.
    pub fn with_routines(mut self) -> Self {
        self.enable_routines = true;
        self
    }

    /// Add pre-recorded HTTP exchanges for the `ReplayingHttpInterceptor`.
    pub fn with_http_exchanges(mut self, exchanges: Vec<HttpExchange>) -> Self {
        self.http_exchanges = exchanges;
        self
    }
}

impl Default for TestRigBuilder {
    fn default() -> Self {
        Self::new()
    }
}
