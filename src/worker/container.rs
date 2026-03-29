//! Worker runtime: the main execution loop inside a container.
//!
//! Reuses the existing `Reasoning` and `SafetyLayer` infrastructure but
//! connects to the orchestrator for LLM calls instead of calling APIs directly.
//! Streams real-time events (message, tool_use, tool_result, result) through
//! the orchestrator's job event pipeline for UI visibility.
//!
//! Uses the shared `AgenticLoop` engine via `ContainerDelegate`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::agentic_loop::{AgenticLoopConfig, LoopOutcome, truncate_for_preview};
use crate::config::SafetyConfig;
use crate::error::WorkerError;
use crate::llm::{ChatMessage, LlmProvider, Reasoning, ReasoningContext};
use crate::safety::SafetyLayer;
use crate::tools::ToolRegistry;
use crate::tools::builtin::worker_remote_tool_proxy::register_worker_remote_tool_proxies;
use crate::worker::api::{
    CompletionReport, JobEventPayload, JobEventType, StatusUpdate, WorkerHttpClient, WorkerState,
};
use crate::worker::proxy_llm::ProxyLlmProvider;

mod delegate;

use delegate::ContainerDelegate;

/// Configuration for the worker runtime.
pub struct WorkerConfig {
    pub job_id: Uuid,
    pub orchestrator_url: String,
    pub max_iterations: u32,
    pub timeout: Duration,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            job_id: Uuid::nil(),
            orchestrator_url: String::new(),
            max_iterations: 50,
            timeout: Duration::from_secs(600),
        }
    }
}

/// The worker runtime runs inside a Docker container.
///
/// It connects to the orchestrator over HTTP, fetches its job description,
/// then runs a tool execution loop until the job is complete. Events are
/// streamed to the orchestrator so the UI can show real-time progress.
pub struct WorkerRuntime {
    config: WorkerConfig,
    client: Arc<WorkerHttpClient>,
    llm: Arc<dyn LlmProvider>,
    safety: Arc<SafetyLayer>,
    tools: Arc<ToolRegistry>,
    toolset_instructions: Vec<String>,
    /// Credentials fetched from the orchestrator, injected into child processes
    /// via `Command::envs()` rather than mutating the global process environment.
    ///
    /// Wrapped in `Arc` to avoid deep-cloning the map on every tool invocation.
    extra_env: Arc<HashMap<String, String>>,
}

enum WorkerExecutionResult {
    Outcome(LoopOutcome),
    Failed(crate::error::Error),
    TimedOut,
}

/// Heading used to tag hosted remote-tool guidance messages in reasoning context.
pub(crate) const HOSTED_GUIDANCE_HEADING: &str = "Hosted remote-tool guidance";

/// Canonical merge of every tool the worker can offer to the LLM.
///
/// Currently a pass-through to `ToolRegistry::tool_definitions`, but
/// centralised here so the reasoning layer has a single call site to
/// extend with sorting, filtering, or deduplication if merge policy
/// evolves.
async fn available_tool_definitions(tools: &ToolRegistry) -> Vec<crate::llm::ToolDefinition> {
    tools.tool_definitions().await
}

impl WorkerRuntime {
    /// Create a worker runtime from an explicit, pre-validated HTTP client.
    pub fn new(config: WorkerConfig, client: Arc<WorkerHttpClient>) -> Self {
        let llm: Arc<dyn LlmProvider> = Arc::new(ProxyLlmProvider::new(
            Arc::clone(&client),
            "proxied".to_string(),
        ));

        let safety = Arc::new(SafetyLayer::new(&SafetyConfig {
            max_output_length: 100_000,
            injection_check_enabled: true,
        }));

        let tools = Self::build_tools();

        Self {
            config,
            client,
            llm,
            safety,
            tools,
            toolset_instructions: Vec::new(),
            extra_env: Arc::new(HashMap::new()),
        }
    }

    /// Create a worker runtime using the environment-backed worker token.
    ///
    /// Reads `IRONCLAW_WORKER_TOKEN` from the environment for auth.
    pub fn from_env(config: WorkerConfig) -> Result<Self, WorkerError> {
        let client = Arc::new(WorkerHttpClient::from_env(
            config.orchestrator_url.clone(),
            config.job_id,
        )?);

        Ok(Self::new(config, client))
    }

    fn build_tools() -> Arc<ToolRegistry> {
        let tools = Arc::new(ToolRegistry::new());
        tools.register_container_tools();
        tools
    }

    /// Run the worker until the job is complete or an error occurs.
    pub async fn run(mut self) -> Result<(), WorkerError> {
        tracing::info!("Worker starting for job {}", self.config.job_id);

        let job = match self.client.get_job().await {
            Ok(job) => job,
            Err(error) => return self.fail_pre_loop("fetch job", error).await,
        };
        tracing::info!(
            "Received job: {} - {}",
            job.title,
            truncate_for_preview(&job.description, 100)
        );

        if let Err(error) = self.hydrate_credentials().await {
            return self.fail_pre_loop("hydrate credentials", error).await;
        }
        self.toolset_instructions = match self.register_remote_tools().await {
            Ok(toolset_instructions) => toolset_instructions,
            Err(error) => {
                tracing::warn!(
                    job_id = %self.config.job_id,
                    error = %error,
                    "Failed to fetch hosted remote-tool catalogue; continuing with container-local tools only"
                );
                Vec::new()
            }
        };
        if let Err(error) = self
            .report_worker_status(
                WorkerState::InProgress,
                Some("Worker started, beginning execution".to_string()),
                0,
            )
            .await
        {
            return self.fail_pre_loop("report initial status", error).await;
        }

        let iteration_tracker = Arc::new(Mutex::new(0u32));
        let execution = match tokio::time::timeout(
            self.config.timeout,
            self.run_job_loop(&job, Arc::clone(&iteration_tracker)),
        )
        .await
        {
            Ok(Ok(outcome)) => WorkerExecutionResult::Outcome(outcome),
            Ok(Err(error)) => WorkerExecutionResult::Failed(error),
            Err(_) => WorkerExecutionResult::TimedOut,
        };

        let iterations = *iteration_tracker.lock().await;
        self.report_completion(execution, iterations).await?;

        Ok(())
    }

    async fn hydrate_credentials(&mut self) -> Result<(), WorkerError> {
        let credentials = self.client.fetch_credentials().await?;
        let mut env_map = HashMap::new();
        for cred in &credentials {
            env_map.insert(cred.env_var.clone(), cred.value.clone());
        }
        self.extra_env = Arc::new(env_map);

        if !credentials.is_empty() {
            tracing::info!(
                "Fetched {} credential(s) for child process injection",
                credentials.len()
            );
        }

        Ok(())
    }

    async fn fail_pre_loop<T>(&self, stage: &str, error: WorkerError) -> Result<T, WorkerError> {
        tracing::error!(
            job_id = %self.config.job_id,
            stage,
            error = %error,
            "Worker failed before the execution loop started"
        );

        if let Err(report_error) = self
            .report_worker_status(
                WorkerState::Failed,
                Some("pre-loop failure".to_string()),
                100,
            )
            .await
        {
            tracing::warn!(
                job_id = %self.config.job_id,
                stage,
                error = %report_error,
                "Failed to emit terminal pre-loop worker status"
            );
        }

        if let Err(report_error) = self.report_failure(0, "Worker failed during startup").await {
            tracing::warn!(
                job_id = %self.config.job_id,
                stage,
                error = %report_error,
                "Failed to emit terminal pre-loop completion"
            );
        }

        Err(error)
    }

    async fn report_worker_status(
        &self,
        state: WorkerState,
        message: Option<String>,
        iteration: u32,
    ) -> Result<(), WorkerError> {
        self.client
            .report_status(&StatusUpdate::new(state, message, iteration))
            .await
    }

    async fn run_job_loop(
        &self,
        job: &crate::worker::api::JobDescription,
        iteration_tracker: Arc<Mutex<u32>>,
    ) -> Result<LoopOutcome, crate::error::Error> {
        let reasoning = Reasoning::new(Arc::clone(&self.llm));
        let mut reason_ctx = self.build_reasoning_context(job).await;

        let delegate = ContainerDelegate {
            client: Arc::clone(&self.client),
            safety: Arc::clone(&self.safety),
            tools: Arc::clone(&self.tools),
            extra_env: Arc::clone(&self.extra_env),
            last_output: Mutex::new(String::new()),
            iteration_tracker,
        };

        let config = AgenticLoopConfig {
            max_iterations: self.config.max_iterations as usize,
            enable_tool_intent_nudge: true,
            max_tool_intent_nudges: 2,
        };

        crate::agent::agentic_loop::run_agentic_loop(
            &delegate,
            &reasoning,
            &mut reason_ctx,
            &config,
        )
        .await
    }

    async fn build_reasoning_context(
        &self,
        job: &crate::worker::api::JobDescription,
    ) -> ReasoningContext {
        let mut reason_ctx = ReasoningContext::new().with_job(&job.description);
        if !self.toolset_instructions.is_empty() {
            reason_ctx.messages.push(ChatMessage::system(format!(
                "{HOSTED_GUIDANCE_HEADING}:\n\n{}",
                self.toolset_instructions.join("\n")
            )));
        }
        reason_ctx.messages.push(ChatMessage::system(format!(
            r#"You are an autonomous agent running inside a Docker container.

Job: {}
Description: {}

Use the available tools listed below to inspect the current capability surface.
This toolset may include container-local tools and, when the remote catalogue
loads, orchestrator-proxied remote tools.
Work independently to complete this job. Report when done."#,
            job.title, job.description
        )));
        reason_ctx.available_tools = available_tool_definitions(&self.tools).await;
        reason_ctx
    }

    async fn report_completion(
        &self,
        execution: WorkerExecutionResult,
        iterations: u32,
    ) -> Result<(), WorkerError> {
        match execution {
            WorkerExecutionResult::Outcome(LoopOutcome::Response(output)) => {
                tracing::info!("Worker completed job {} successfully", self.config.job_id);
                self.post_event(
                    JobEventType::Result,
                    serde_json::json!({
                        "success": true,
                        "message": truncate_for_preview(&output, 2000),
                    }),
                )
                .await;
                self.client
                    .report_complete(&CompletionReport {
                        success: true,
                        message: Some(output),
                        iterations,
                    })
                    .await
            }
            WorkerExecutionResult::Outcome(LoopOutcome::MaxIterations) => {
                let msg = format!("max iterations ({}) exceeded", self.config.max_iterations);
                tracing::warn!("Worker failed for job {}: {}", self.config.job_id, msg);
                self.report_failure(iterations, &format!("Execution failed: {}", msg))
                    .await
            }
            WorkerExecutionResult::Outcome(LoopOutcome::Stopped | LoopOutcome::NeedApproval(_)) => {
                tracing::info!("Worker for job {} stopped", self.config.job_id);
                self.post_event(
                    JobEventType::Result,
                    serde_json::json!({
                        "success": false,
                        "message": "Execution stopped",
                        "iterations": iterations,
                    }),
                )
                .await;
                self.client
                    .report_complete(&CompletionReport {
                        success: false,
                        message: Some("Execution stopped".to_string()),
                        iterations,
                    })
                    .await
            }
            WorkerExecutionResult::Failed(error) => {
                tracing::error!("Worker failed for job {}: {}", self.config.job_id, error);
                self.report_failure(iterations, "Execution failed").await
            }
            WorkerExecutionResult::TimedOut => {
                tracing::warn!("Worker timed out for job {}", self.config.job_id);
                self.report_failure(iterations, "Execution timed out").await
            }
        }
    }

    async fn report_failure(&self, iterations: u32, message: &str) -> Result<(), WorkerError> {
        self.post_event(
            JobEventType::Result,
            serde_json::json!({
                "success": false,
                "message": message,
            }),
        )
        .await;
        self.client
            .report_complete(&CompletionReport {
                success: false,
                message: Some(message.to_string()),
                iterations,
            })
            .await
    }

    /// Post a job event to the orchestrator (fire-and-forget).
    async fn post_event(&self, event_type: JobEventType, data: serde_json::Value) {
        self.client
            .post_event(&JobEventPayload { event_type, data })
            .await;
    }

    async fn register_remote_tools(&self) -> Result<Vec<String>, WorkerError> {
        let remote_catalog = self.client.get_remote_tool_catalog().await?;
        let remote_tool_count = remote_catalog.tools.len();
        let catalog_version = remote_catalog.catalog_version;
        let toolset_instructions = remote_catalog.toolset_instructions;
        register_worker_remote_tool_proxies(
            &self.tools,
            Arc::clone(&self.client),
            remote_catalog.tools,
        );
        tracing::info!(
            job_id = %self.config.job_id,
            catalog_version,
            remote_tool_count,
            "Registered hosted remote tools from orchestrator catalog"
        );
        Ok(toolset_instructions)
    }

    #[cfg(test)]
    async fn register_remote_tools_with_degraded_startup(&self) {
        if let Err(error) = self.register_remote_tools().await {
            tracing::warn!(
                job_id = %self.config.job_id,
                error = %error,
                "Failed to fetch hosted remote-tool catalogue; continuing with container-local tools only"
            );
        }
    }
}

#[cfg(test)]
mod tests;
