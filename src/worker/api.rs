//! HTTP client for worker-to-orchestrator communication.
//!
//! Every request includes a bearer token from `IRONCLAW_WORKER_TOKEN` env var.
//! The orchestrator validates this token is scoped to the correct job.

use serde::Serialize;
use uuid::Uuid;

use crate::error::WorkerError;
use crate::llm::{
    CompletionRequest, CompletionResponse, ToolCompletionRequest, ToolCompletionResponse,
};
use crate::tools::ToolOutput;

mod types;

use error_mapping::map_remote_tool_status;

pub use types::{
    CompletionReport, CredentialResponse, FinishReason as ProxyFinishReason, JobDescription,
    JobEventPayload, JobEventType, PromptResponse, ProxyCompletionRequest, ProxyCompletionResponse,
    ProxyToolCompletionRequest, ProxyToolCompletionResponse, REMOTE_TOOL_CATALOG_PATH,
    REMOTE_TOOL_CATALOG_ROUTE, REMOTE_TOOL_EXECUTE_PATH, REMOTE_TOOL_EXECUTE_ROUTE,
    RemoteToolCatalogResponse, RemoteToolExecutionRequest, RemoteToolExecutionResponse,
    StatusUpdate, WORKER_COMPLETE_PATH, WORKER_COMPLETE_ROUTE, WORKER_CREDENTIALS_PATH,
    WORKER_CREDENTIALS_ROUTE, WORKER_EVENT_PATH, WORKER_EVENT_ROUTE, WORKER_HEALTH_ROUTE,
    WORKER_JOB_PATH, WORKER_JOB_ROUTE, WORKER_LLM_COMPLETE_PATH, WORKER_LLM_COMPLETE_ROUTE,
    WORKER_LLM_COMPLETE_WITH_TOOLS_PATH, WORKER_LLM_COMPLETE_WITH_TOOLS_ROUTE, WORKER_PROMPT_PATH,
    WORKER_PROMPT_ROUTE, WORKER_STATUS_PATH, WORKER_STATUS_ROUTE, WorkerState,
};
/// HTTP client that a container worker uses to talk to the orchestrator.
pub struct WorkerHttpClient {
    client: reqwest::Client,
    orchestrator_url: String,
    job_id: Uuid,
    token: String,
}

const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
impl WorkerHttpClient {
    /// Create a new client from environment.
    ///
    /// Reads `IRONCLAW_WORKER_TOKEN` from the environment.
    pub fn from_env(orchestrator_url: String, job_id: Uuid) -> Result<Self, WorkerError> {
        let token =
            std::env::var("IRONCLAW_WORKER_TOKEN").map_err(|_| WorkerError::MissingToken)?;

        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .map_err(|e| WorkerError::ConnectionFailed {
                    url: orchestrator_url.clone(),
                    reason: format!("failed to build HTTP client: {}", e),
                })?,
            orchestrator_url: orchestrator_url.trim_end_matches('/').to_string(),
            job_id,
            token,
        })
    }

    /// Create with an explicit token (for testing).
    pub fn new(orchestrator_url: String, job_id: Uuid, token: String) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_default(),
            orchestrator_url: orchestrator_url.trim_end_matches('/').to_string(),
            job_id,
            token,
        }
    }

    /// Get the base orchestrator URL.
    pub fn orchestrator_url(&self) -> &str {
        &self.orchestrator_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}/worker/{}/{}", self.orchestrator_url, self.job_id, path)
    }

    async fn send_post_json<B: Serialize>(
        &self,
        path: &str,
        body: &B,
        context: &str,
    ) -> Result<reqwest::Response, WorkerError> {
        self.client
            .post(self.url(path))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .map_err(|e| WorkerError::LlmProxyFailed {
                reason: format!("{}: {}", context, e),
            })
    }

    /// Send a GET request, check the status, and deserialize the JSON body.
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        context: &str,
    ) -> Result<T, WorkerError> {
        let resp = self
            .client
            .get(self.url(path))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| WorkerError::ConnectionFailed {
                url: self.orchestrator_url.clone(),
                reason: e.to_string(),
            })?;

        if !resp.status().is_success() {
            return Err(WorkerError::OrchestratorRejected {
                job_id: self.job_id,
                reason: format!("{} returned {}", context, resp.status()),
            });
        }

        resp.json().await.map_err(|e| WorkerError::LlmProxyFailed {
            reason: format!("{}: failed to parse response: {}", context, e),
        })
    }

    /// Send a POST request with a JSON body, check the status, and deserialize the response.
    async fn post_json<B: Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
        context: &str,
    ) -> Result<T, WorkerError> {
        let resp = self.send_post_json(path, body, context).await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(WorkerError::LlmProxyFailed {
                reason: format!("{}: orchestrator returned {}: {}", context, status, body),
            });
        }

        resp.json().await.map_err(|e| WorkerError::LlmProxyFailed {
            reason: format!("{}: failed to parse response: {}", context, e),
        })
    }

    /// Fetch the job description from the orchestrator.
    pub async fn get_job(&self) -> Result<JobDescription, WorkerError> {
        self.get_json(WORKER_JOB_PATH, "GET /job").await
    }

    /// Proxy an LLM completion request through the orchestrator.
    pub async fn llm_complete(
        &self,
        request: &CompletionRequest,
    ) -> Result<CompletionResponse, WorkerError> {
        let proxy_req = ProxyCompletionRequest {
            messages: request.messages.clone(),
            model: request.model.clone(),
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            stop_sequences: request.stop_sequences.clone(),
        };

        let proxy_resp: ProxyCompletionResponse = self
            .post_json(WORKER_LLM_COMPLETE_PATH, &proxy_req, "LLM complete")
            .await?;

        Ok(CompletionResponse {
            content: proxy_resp.content,
            input_tokens: proxy_resp.input_tokens,
            output_tokens: proxy_resp.output_tokens,
            finish_reason: proxy_resp.finish_reason.into(),
            cache_read_input_tokens: proxy_resp.cache_read_input_tokens,
            cache_creation_input_tokens: proxy_resp.cache_creation_input_tokens,
        })
    }

    /// Proxy an LLM tool completion request through the orchestrator.
    pub async fn llm_complete_with_tools(
        &self,
        request: &ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, WorkerError> {
        let proxy_req = ProxyToolCompletionRequest {
            messages: request.messages.clone(),
            tools: request.tools.clone(),
            model: request.model.clone(),
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            tool_choice: request.tool_choice.clone(),
        };

        let proxy_resp: ProxyToolCompletionResponse = self
            .post_json(
                WORKER_LLM_COMPLETE_WITH_TOOLS_PATH,
                &proxy_req,
                "LLM tool complete",
            )
            .await?;

        Ok(ToolCompletionResponse {
            content: proxy_resp.content,
            tool_calls: proxy_resp.tool_calls,
            input_tokens: proxy_resp.input_tokens,
            output_tokens: proxy_resp.output_tokens,
            finish_reason: proxy_resp.finish_reason.into(),
            cache_read_input_tokens: proxy_resp.cache_read_input_tokens,
            cache_creation_input_tokens: proxy_resp.cache_creation_input_tokens,
        })
    }

    /// Fetch the hosted-visible orchestrator-owned remote tool catalog.
    pub async fn get_remote_tool_catalog(&self) -> Result<RemoteToolCatalogResponse, WorkerError> {
        self.get_json(REMOTE_TOOL_CATALOG_PATH, "GET /tools/catalog")
            .await
    }

    /// Execute an orchestrator-owned hosted remote tool.
    pub async fn execute_remote_tool(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> Result<ToolOutput, WorkerError> {
        let proxy_req = RemoteToolExecutionRequest {
            tool_name: tool_name.to_string(),
            params: params.clone(),
        };

        let resp = self
            .send_post_json(
                REMOTE_TOOL_EXECUTE_PATH,
                &proxy_req,
                "Remote tool execution",
            )
            .await?;

        if !resp.status().is_success() {
            return Err(map_remote_tool_status(resp).await);
        }

        let proxy_resp: RemoteToolExecutionResponse =
            resp.json().await.map_err(|e| WorkerError::LlmProxyFailed {
                reason: format!("Remote tool execution: failed to parse response: {}", e),
            })?;

        Ok(proxy_resp.output)
    }

    /// Report status to the orchestrator.
    pub async fn report_status(&self, update: &StatusUpdate) -> Result<(), WorkerError> {
        let resp = self
            .client
            .post(self.url(WORKER_STATUS_PATH))
            .bearer_auth(&self.token)
            .json(update)
            .send()
            .await
            .map_err(|e| WorkerError::ConnectionFailed {
                url: self.orchestrator_url.clone(),
                reason: e.to_string(),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(WorkerError::OrchestratorRejected {
                job_id: self.job_id,
                reason: format!("status endpoint returned {}: {}", status, body),
            });
        }

        Ok(())
    }

    /// Report a non-terminal status update without failing the worker on rejection.
    pub async fn report_status_lossy(&self, update: &StatusUpdate) {
        if let Err(error) = self.report_status(update).await {
            tracing::warn!(
                job_id = %self.job_id,
                state = %update.state,
                iteration = update.iteration,
                error = %error,
                "Worker status report failed"
            );
        }
    }

    /// Post a job event to the orchestrator (fire-and-forget style, logs on failure).
    pub async fn post_event(&self, payload: &JobEventPayload) {
        let resp = self
            .client
            .post(self.url(WORKER_EVENT_PATH))
            .bearer_auth(&self.token)
            .json(payload)
            .send()
            .await;

        match resp {
            Ok(r) if !r.status().is_success() => {
                tracing::debug!(
                    job_id = %self.job_id,
                    event_type = %payload.event_type,
                    status = %r.status(),
                    "Job event POST rejected"
                );
            }
            Err(e) => {
                tracing::debug!(
                    job_id = %self.job_id,
                    event_type = %payload.event_type,
                    "Job event POST failed: {}", e
                );
            }
            _ => {}
        }
    }

    /// Poll the orchestrator for a follow-up prompt.
    ///
    /// Returns `None` if no prompt is available (204 No Content).
    pub async fn poll_prompt(&self) -> Result<Option<PromptResponse>, WorkerError> {
        let resp = self
            .client
            .get(self.url(WORKER_PROMPT_PATH))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| WorkerError::ConnectionFailed {
                url: self.orchestrator_url.clone(),
                reason: e.to_string(),
            })?;

        if resp.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }

        if !resp.status().is_success() {
            return Err(WorkerError::OrchestratorRejected {
                job_id: self.job_id,
                reason: format!("prompt endpoint returned {}", resp.status()),
            });
        }

        let prompt: PromptResponse =
            resp.json().await.map_err(|e| WorkerError::LlmProxyFailed {
                reason: format!("failed to parse prompt response: {}", e),
            })?;

        Ok(Some(prompt))
    }

    /// Fetch credentials granted to this job from the orchestrator.
    ///
    /// Returns an empty vec if no credentials are granted (204 No Content)
    /// or if the endpoint returns 404. The caller should set each credential
    /// as an environment variable before starting the execution loop.
    pub async fn fetch_credentials(&self) -> Result<Vec<CredentialResponse>, WorkerError> {
        let resp = self
            .client
            .get(self.url(WORKER_CREDENTIALS_PATH))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| WorkerError::ConnectionFailed {
                url: self.orchestrator_url.clone(),
                reason: e.to_string(),
            })?;

        // 204 or 404 means no credentials granted, not an error
        if resp.status() == reqwest::StatusCode::NO_CONTENT
            || resp.status() == reqwest::StatusCode::NOT_FOUND
        {
            return Ok(vec![]);
        }

        if !resp.status().is_success() {
            return Err(WorkerError::SecretResolveFailed {
                secret_name: "(all)".to_string(),
                reason: format!("credentials endpoint returned {}", resp.status()),
            });
        }

        resp.json()
            .await
            .map_err(|e| WorkerError::SecretResolveFailed {
                secret_name: "(all)".to_string(),
                reason: format!("failed to parse credentials response: {}", e),
            })
    }

    /// Signal job completion to the orchestrator.
    pub async fn report_complete(&self, report: &CompletionReport) -> Result<(), WorkerError> {
        let _: serde_json::Value = self
            .post_json(WORKER_COMPLETE_PATH, report, "report complete")
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;

mod error_mapping;
