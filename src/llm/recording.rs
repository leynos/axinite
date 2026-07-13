//! Live trace recording mode.
//!
//! Wraps any [`LlmProvider`] and captures every LLM interaction into
//! the trace fixture format used by `TraceLlm` for deterministic E2E
//! testing. Recorded traces can be replayed later via `TraceLlm`.
//!
//! The trace includes:
//! - **Memory snapshot**: workspace documents captured before the first LLM call
//! - **HTTP exchanges**: all outgoing HTTP request/response pairs from tools
//! - **Steps**: user inputs, LLM responses (text/tool_calls), and expected tool
//!   results for verifying tool output during replay
//!
//! Enable by setting `IRONCLAW_RECORD_TRACE=1` at runtime.

use std::path::PathBuf;
use std::sync::Arc;

use rust_decimal::Decimal;
use tokio::sync::Mutex;

use crate::llm::error::LlmError;
use crate::llm::provider::{
    ChatMessage, CompletionRequest, CompletionResponse, LlmProvider, ModelMetadata, Role,
    ToolCompletionRequest, ToolCompletionResponse,
};

mod http_interceptor;
mod trace_format;

pub use http_interceptor::{
    HttpInterceptor, HttpInterceptorFuture, NativeHttpInterceptor, RecordingHttpInterceptor,
    ReplayingHttpInterceptor,
};
pub use trace_format::{
    ExpectedToolResult, HttpExchange, HttpExchangeRequest, HttpExchangeResponse,
    MemorySnapshotEntry, RequestHint, TraceFile, TraceResponse, TraceStep, TraceToolCall,
};

// ── RecordingLlm ───────────────────────────────────────────────────

/// LLM provider decorator that records interactions into a trace file.
pub struct RecordingLlm {
    inner: Arc<dyn LlmProvider>,
    steps: Mutex<Vec<TraceStep>>,
    prev_message_count: Mutex<usize>,
    output_path: PathBuf,
    model_name: String,
    memory_snapshot: Mutex<Vec<MemorySnapshotEntry>>,
    http_interceptor: Arc<RecordingHttpInterceptor>,
}

impl RecordingLlm {
    /// Wrap a provider for recording.
    pub fn new(inner: Arc<dyn LlmProvider>, output_path: PathBuf, model_name: String) -> Self {
        Self {
            inner,
            steps: Mutex::new(Vec::new()),
            prev_message_count: Mutex::new(0),
            output_path,
            model_name,
            memory_snapshot: Mutex::new(Vec::new()),
            http_interceptor: Arc::new(RecordingHttpInterceptor::new()),
        }
    }

    /// Create from environment variables if recording is enabled.
    ///
    /// - `IRONCLAW_RECORD_TRACE` — any non-empty value enables recording
    /// - `IRONCLAW_TRACE_OUTPUT` — file path (default: `./trace_{timestamp}.json`)
    /// - `IRONCLAW_TRACE_MODEL_NAME` — model_name field (default: `recorded-{inner.model_name()}`)
    pub fn from_env(inner: Arc<dyn LlmProvider>) -> Option<Arc<Self>> {
        let enabled = std::env::var("IRONCLAW_RECORD_TRACE")
            .ok()
            .filter(|v| !v.is_empty());
        enabled?;

        let output_path = std::env::var("IRONCLAW_TRACE_OUTPUT")
            .ok()
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let ts = chrono::Local::now().format("%Y%m%dT%H%M%S");
                PathBuf::from(format!("trace_{ts}.json"))
            });

        let model_name = std::env::var("IRONCLAW_TRACE_MODEL_NAME")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| format!("recorded-{}", inner.model_name()));

        tracing::info!(
            output = %output_path.display(),
            model = %model_name,
            "LLM trace recording enabled"
        );

        Some(Arc::new(Self::new(inner, output_path, model_name)))
    }

    /// Get the HTTP interceptor for wiring into tools.
    ///
    /// Pass this to `JobContext` or `HttpTool` so outgoing HTTP requests
    /// are recorded into the trace.
    pub fn http_interceptor(&self) -> Arc<dyn HttpInterceptor> {
        Arc::clone(&self.http_interceptor) as Arc<dyn HttpInterceptor>
    }

    /// Snapshot all memory documents from a workspace.
    ///
    /// Call this once after creation, before the agent starts processing.
    pub async fn snapshot_memory(&self, workspace: &crate::workspace::Workspace) {
        match workspace.list_all().await {
            Ok(paths) => {
                let mut snapshot = self.memory_snapshot.lock().await;
                for path in paths {
                    match workspace.read(&path).await {
                        Ok(doc) => {
                            snapshot.push(MemorySnapshotEntry {
                                path: doc.path,
                                content: doc.content,
                            });
                        }
                        Err(e) => {
                            tracing::debug!(path = %path, error = %e, "Skipped memory doc in snapshot");
                        }
                    }
                }
                tracing::info!(
                    documents = snapshot.len(),
                    "Captured memory snapshot for trace recording"
                );
            }
            Err(e) => {
                tracing::warn!("Failed to snapshot memory for trace recording: {}", e);
            }
        }
    }

    /// Flush accumulated steps, memory snapshot, and HTTP exchanges to the output file.
    pub async fn flush(&self) -> Result<(), std::io::Error> {
        let steps = self.steps.lock().await;
        let memory_snapshot = self.memory_snapshot.lock().await;
        let http_exchanges = self.http_interceptor.take_exchanges().await;

        let trace = TraceFile {
            model_name: self.model_name.clone(),
            memory_snapshot: memory_snapshot.clone(),
            http_exchanges,
            steps: steps.clone(),
        };
        let json = serde_json::to_string_pretty(&trace).map_err(std::io::Error::other)?;
        tokio::fs::write(&self.output_path, json).await?;
        tracing::info!(
            steps = steps.len(),
            memory_docs = memory_snapshot.len(),
            path = %self.output_path.display(),
            "Flushed LLM trace recording"
        );
        Ok(())
    }

    /// Extract new user messages, tool results, and build request hint.
    ///
    /// Returns `(hint, tool_results)` where tool_results are new `Role::Tool`
    /// messages since the last call — these become `expected_tool_results` on
    /// the next step for replay verification.
    async fn capture_new_messages(
        &self,
        messages: &[ChatMessage],
    ) -> (Option<RequestHint>, Vec<ExpectedToolResult>) {
        let mut prev_count = self.prev_message_count.lock().await;
        let current_count = messages.len();
        // After context compaction, the message list may shrink below
        // prev_count.  Clamp to avoid an out-of-bounds slice.
        let start = (*prev_count).min(current_count);

        let new_messages = &messages[start..];

        // Emit UserInput steps for new user messages
        let new_user_messages: Vec<&ChatMessage> = new_messages
            .iter()
            .filter(|m| m.role == Role::User)
            .collect();

        if !new_user_messages.is_empty() {
            let mut steps = self.steps.lock().await;
            for msg in &new_user_messages {
                steps.push(TraceStep {
                    request_hint: None,
                    response: TraceResponse::UserInput {
                        content: msg.content.clone(),
                    },
                    expected_tool_results: Vec::new(),
                });
            }
        }

        // Capture new tool result messages for expected_tool_results
        let tool_results: Vec<ExpectedToolResult> = new_messages
            .iter()
            .filter(|m| m.role == Role::Tool)
            .map(|m| ExpectedToolResult {
                tool_call_id: m.tool_call_id.clone().unwrap_or_default(),
                name: m.name.clone().unwrap_or_default(),
                content: m.content.clone(),
            })
            .collect();

        *prev_count = current_count;

        // Build request hint from last user message
        let hint = messages
            .iter()
            .rev()
            .find(|m| m.role == Role::User)
            .map(|msg| {
                let hint_text = if msg.content.len() > 80 {
                    let mut end = 80;
                    while end > 0 && !msg.content.is_char_boundary(end) {
                        end -= 1;
                    }
                    msg.content[..end].to_string()
                } else {
                    msg.content.clone()
                };
                RequestHint {
                    last_user_message_contains: Some(hint_text),
                    min_message_count: Some(current_count),
                }
            });

        (hint, tool_results)
    }
}

impl crate::llm::NativeLlmProvider for RecordingLlm {
    fn model_name(&self) -> &str {
        self.inner.model_name()
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        self.inner.cost_per_token()
    }

    fn cache_write_multiplier(&self) -> Decimal {
        self.inner.cache_write_multiplier()
    }

    fn cache_read_discount(&self) -> Decimal {
        self.inner.cache_read_discount()
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let (hint, tool_results) = self.capture_new_messages(&request.messages).await;
        let response = self.inner.complete(request).await?;

        self.steps.lock().await.push(TraceStep {
            request_hint: hint,
            response: TraceResponse::Text {
                content: response.content.clone(),
                input_tokens: response.input_tokens,
                output_tokens: response.output_tokens,
            },
            expected_tool_results: tool_results,
        });

        Ok(response)
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let (hint, tool_results) = self.capture_new_messages(&request.messages).await;
        let response = self.inner.complete_with_tools(request).await?;

        let step = if response.tool_calls.is_empty() {
            TraceStep {
                request_hint: hint,
                response: TraceResponse::Text {
                    content: response.content.clone().unwrap_or_default(),
                    input_tokens: response.input_tokens,
                    output_tokens: response.output_tokens,
                },
                expected_tool_results: tool_results,
            }
        } else {
            TraceStep {
                request_hint: hint,
                response: TraceResponse::ToolCalls {
                    tool_calls: response
                        .tool_calls
                        .iter()
                        .map(|tc| TraceToolCall {
                            id: tc.id.clone(),
                            name: tc.name.clone(),
                            arguments: tc.arguments.clone(),
                        })
                        .collect(),
                    input_tokens: response.input_tokens,
                    output_tokens: response.output_tokens,
                },
                expected_tool_results: tool_results,
            }
        };

        self.steps.lock().await.push(step);
        Ok(response)
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        self.inner.list_models().await
    }

    async fn model_metadata(&self) -> Result<ModelMetadata, LlmError> {
        self.inner.model_metadata().await
    }

    fn effective_model_name(&self, requested_model: Option<&str>) -> String {
        self.inner.effective_model_name(requested_model)
    }

    fn active_model_name(&self) -> String {
        self.inner.active_model_name()
    }

    fn set_model(&self, model: &str) -> Result<(), LlmError> {
        self.inner.set_model(model)
    }

    fn calculate_cost(&self, input_tokens: u32, output_tokens: u32) -> Decimal {
        self.inner.calculate_cost(input_tokens, output_tokens)
    }
}

#[cfg(test)]
mod tests;
