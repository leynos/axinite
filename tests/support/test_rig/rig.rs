//! `TestRig` — the central test harness for end-to-end agent tests.
//!
//! A `TestRig` wires together an in-process `TestChannel`, an
//! `InstrumentedLlm`, and (when the `libsql` feature is enabled) a
//! database and workspace handle. Tests obtain a `TestRig` via
//! `TestRigBuilder` and drive the agent by calling `send_message`,
//! `wait_for_responses`, or the trace-replay helpers.
//!
//! Synchronous status-event accessors (`tool_calls_completed`,
//! `captured_status_events`) use `try_lock` and are appropriate when
//! the agent has already settled. The async variants
//! (`tool_calls_completed_async`, `captured_status_events_async`) await
//! the mutex and should be preferred inside polling loops where events
//! may still be arriving.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ironclaw::channels::{OutgoingResponse, StatusUpdate};
use ironclaw::db::Database;
use ironclaw::error::LlmError;

use crate::support::assertions::verify_expects;
use crate::support::instrumented_llm::InstrumentedLlm;
use crate::support::metrics::{ToolInvocation, TraceMetrics};
use crate::support::test_channel::TestChannel;
use crate::support::trace_provider::TraceLlm;
use crate::support::trace_types::LlmTrace;

/// A running test agent with methods to inject messages and inspect results.
pub struct TestRig {
    /// The test channel for sending messages and reading captures.
    pub(super) channel: Arc<TestChannel>,
    /// Instrumented LLM for collecting token/call metrics.
    pub(super) instrumented_llm: Arc<InstrumentedLlm>,
    /// When the rig was created (for wall-time measurement).
    pub(super) start_time: Instant,
    /// Maximum tool-call iterations per agentic loop.
    pub(super) max_tool_iterations: usize,
    /// Handle to the background agent task.
    pub(super) agent_handle: Option<tokio::task::JoinHandle<()>>,
    /// Database handle for direct queries in tests.
    #[cfg(feature = "libsql")]
    pub(super) db: Arc<dyn Database>,
    /// Workspace handle for direct memory operations in tests.
    #[cfg(feature = "libsql")]
    pub(super) workspace: Option<Arc<ironclaw::workspace::Workspace>>,
    /// The underlying TraceLlm for inspecting captured requests.
    #[cfg(feature = "libsql")]
    pub(super) trace_llm: Option<Arc<TraceLlm>>,
    /// Temp directory guard -- keeps the libSQL database file alive.
    #[cfg(feature = "libsql")]
    pub(super) _temp_dir: tempfile::TempDir,
}

impl TestRig {
    /// Inject a user message into the agent.
    pub async fn send_message(&self, content: &str) {
        self.channel.send_message(content).await;
    }

    /// Inject a raw `IncomingMessage` (for tests that need attachments, etc.).
    pub async fn send_incoming(&self, msg: ironclaw::channels::IncomingMessage) {
        self.channel.send_incoming(msg).await;
    }

    /// Return all message lists that were sent to the LLM provider.
    pub fn captured_llm_requests(&self) -> Result<Vec<Vec<ironclaw::llm::ChatMessage>>, LlmError> {
        self.trace_llm
            .as_ref()
            .map(|trace_llm| trace_llm.captured_requests())
            .unwrap_or_else(|| {
                Err(LlmError::RequestFailed {
                    provider: "test-rig".to_string(),
                    reason: "TraceLlm not available; built without tracing".to_string(),
                })
            })
    }

    /// Wait until at least `n` responses have been captured, or `timeout` elapses.
    pub async fn wait_for_responses(&self, n: usize, timeout: Duration) -> Vec<OutgoingResponse> {
        self.channel.wait_for_responses(n, timeout).await
    }

    /// Return the names of all `ToolStarted` events captured so far.
    pub fn tool_calls_started(&self) -> Vec<String> {
        self.channel.tool_calls_started()
    }

    /// Return `(name, success)` for all `ToolCompleted` events captured so far.
    pub fn tool_calls_completed(&self) -> Vec<(String, bool)> {
        self.channel.tool_calls_completed()
    }

    /// Return `(name, success)` for all `ToolCompleted` events captured so far.
    ///
    /// Prefer this accessor while the agent may still be emitting status events.
    pub async fn tool_calls_completed_async(&self) -> Vec<(String, bool)> {
        self.channel.tool_calls_completed_async().await
    }

    /// Return `(name, preview)` for all `ToolResult` events captured so far.
    pub fn tool_results(&self) -> Vec<(String, String)> {
        self.channel.tool_results()
    }

    /// Return `(name, duration_ms)` for all completed tools with timing data.
    pub fn tool_timings(&self) -> Vec<(String, u64)> {
        self.channel.tool_timings()
    }

    /// Return a snapshot of all captured status events.
    pub fn captured_status_events(&self) -> Vec<StatusUpdate> {
        self.channel.captured_status_events()
    }

    /// Number of LLM calls made so far.
    pub fn llm_call_count(&self) -> u32 {
        self.instrumented_llm.call_count()
    }

    /// Total input tokens across all LLM calls.
    pub fn total_input_tokens(&self) -> u32 {
        self.instrumented_llm.total_input_tokens()
    }

    /// Total output tokens across all LLM calls.
    pub fn total_output_tokens(&self) -> u32 {
        self.instrumented_llm.total_output_tokens()
    }

    /// Wall-clock time since rig creation.
    pub fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    /// Collect a complete `TraceMetrics` snapshot from all captured data.
    pub async fn collect_metrics(&self) -> TraceMetrics {
        let completed = self.tool_calls_completed();
        let timings = self.tool_timings();
        let mut timing_iter_by_name: std::collections::HashMap<&str, Vec<u64>> =
            std::collections::HashMap::new();
        for (name, ms) in &timings {
            timing_iter_by_name
                .entry(name.as_str())
                .or_default()
                .push(*ms);
        }

        let tool_invocations: Vec<ToolInvocation> = completed
            .iter()
            .map(|(name, success)| {
                let duration_ms = timing_iter_by_name
                    .get_mut(name.as_str())
                    .and_then(|values| (!values.is_empty()).then(|| values.remove(0)))
                    .unwrap_or(0);
                ToolInvocation {
                    name: name.clone(),
                    duration_ms,
                    success: *success,
                }
            })
            .collect();

        let responses = self.channel.captured_responses();
        let llm_calls = self.instrumented_llm.call_count();
        let response_mentions_iteration_limit = responses.iter().any(|response| {
            let content = response.content.to_lowercase();
            content.contains("iteration limit") || content.contains("iterations")
        });
        let hit_iteration_limit = completed.len() >= self.max_tool_iterations
            || (llm_calls >= self.max_tool_iterations as u32
                && completed.len() < self.max_tool_iterations
                && response_mentions_iteration_limit);
        let turns = responses.len() as u32;

        TraceMetrics {
            wall_time_ms: self.elapsed_ms(),
            llm_calls,
            input_tokens: self.instrumented_llm.total_input_tokens(),
            output_tokens: self.instrumented_llm.total_output_tokens(),
            estimated_cost_usd: self.instrumented_llm.estimated_cost_usd(),
            tool_calls: tool_invocations,
            turns,
            hit_iteration_limit,
            hit_timeout: false,
        }
    }

    /// Run a complete multi-turn trace, injecting user messages from the trace.
    pub async fn run_trace(
        &self,
        trace: &LlmTrace,
        timeout: Duration,
    ) -> Vec<Vec<OutgoingResponse>> {
        let mut all_responses: Vec<Vec<OutgoingResponse>> = Vec::new();
        let mut total_responses = 0usize;
        for turn in &trace.turns {
            self.send_message(&turn.user_input).await;
            let responses = self.wait_for_responses(total_responses + 1, timeout).await;
            let turn_responses: Vec<OutgoingResponse> =
                responses.into_iter().skip(total_responses).collect();
            total_responses += turn_responses.len();
            all_responses.push(turn_responses);
        }
        all_responses
    }

    /// Run a trace, then verify all declarative `expects`.
    pub async fn run_and_verify_trace(
        &self,
        trace: &LlmTrace,
        timeout: Duration,
    ) -> Vec<Vec<OutgoingResponse>> {
        let all_responses = self.run_trace(trace, timeout).await;

        if !trace.expects.is_empty() {
            let all_response_strings: Vec<String> = all_responses
                .iter()
                .flat_map(|turn| turn.iter().map(|response| response.content.clone()))
                .collect();
            let started = self.tool_calls_started();
            let completed = self.tool_calls_completed();
            let results = self.tool_results();
            verify_expects(
                &trace.expects,
                &all_response_strings,
                &started,
                &completed,
                &results,
                "top-level",
            );
        }

        all_responses
    }

    /// Verify top-level `expects` from a trace against already-captured data.
    pub fn verify_trace_expects(&self, trace: &LlmTrace, responses: &[OutgoingResponse]) {
        if trace.expects.is_empty() {
            return;
        }
        let response_strings: Vec<String> = responses
            .iter()
            .map(|response| response.content.clone())
            .collect();
        let started = self.tool_calls_started();
        let completed = self.tool_calls_completed();
        let results = self.tool_results();
        verify_expects(
            &trace.expects,
            &response_strings,
            &started,
            &completed,
            &results,
            "top-level",
        );
    }

    /// Signal the channel to shut down and abort the background agent task.
    pub fn shutdown(mut self) {
        self.channel.signal_shutdown();
        if let Some(handle) = self.agent_handle.take() {
            handle.abort();
        }
    }

    /// Get the database handle for direct queries.
    #[cfg(feature = "libsql")]
    pub fn database(&self) -> &Arc<dyn Database> {
        &self.db
    }

    /// Get the workspace handle for direct memory operations.
    #[cfg(feature = "libsql")]
    pub fn workspace(&self) -> Option<&Arc<ironclaw::workspace::Workspace>> {
        self.workspace.as_ref()
    }

    /// Get the underlying TraceLlm for inspecting captured requests.
    #[cfg(feature = "libsql")]
    pub fn trace_llm(&self) -> Option<&Arc<TraceLlm>> {
        self.trace_llm.as_ref()
    }
}

impl Drop for TestRig {
    fn drop(&mut self) {
        if let Some(handle) = self.agent_handle.take()
            && !handle.is_finished()
        {
            handle.abort();
        }
    }
}
