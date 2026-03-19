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

pub use types::{
    CompletionReport, CredentialResponse, FinishReason as ProxyFinishReason, JobDescription,
    JobEventPayload, PromptResponse, ProxyCompletionRequest, ProxyCompletionResponse,
    ProxyExtensionToolRequest, ProxyExtensionToolResponse, ProxyToolCompletionRequest,
    ProxyToolCompletionResponse, StatusUpdate, WorkerState,
};

/// HTTP client that a container worker uses to talk to the orchestrator.
pub struct WorkerHttpClient {
    client: reqwest::Client,
    orchestrator_url: String,
    job_id: Uuid,
    token: String,
}

impl WorkerHttpClient {
    /// Create a new client from environment.
    ///
    /// Reads `IRONCLAW_WORKER_TOKEN` from the environment.
    pub fn from_env(orchestrator_url: String, job_id: Uuid) -> Result<Self, WorkerError> {
        let token =
            std::env::var("IRONCLAW_WORKER_TOKEN").map_err(|_| WorkerError::MissingToken)?;

        Ok(Self {
            client: reqwest::Client::new(),
            orchestrator_url: orchestrator_url.trim_end_matches('/').to_string(),
            job_id,
            token,
        })
    }

    /// Create with an explicit token (for testing).
    pub fn new(orchestrator_url: String, job_id: Uuid, token: String) -> Self {
        Self {
            client: reqwest::Client::new(),
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
        let resp = self
            .client
            .post(self.url(path))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .map_err(|e| WorkerError::LlmProxyFailed {
                reason: format!("{}: {}", context, e),
            })?;

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
        self.get_json("job", "GET /job").await
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
            .post_json("llm/complete", &proxy_req, "LLM complete")
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
            .post_json("llm/complete_with_tools", &proxy_req, "LLM tool complete")
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

    /// Execute an extension-management tool against the orchestrator-side app state.
    pub async fn execute_extension_tool(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> Result<ToolOutput, WorkerError> {
        let proxy_req = ProxyExtensionToolRequest {
            tool_name: tool_name.to_string(),
            params: params.clone(),
        };

        let proxy_resp: ProxyExtensionToolResponse = self
            .post_json("extension_tool", &proxy_req, "Extension tool execution")
            .await?;

        Ok(proxy_resp.output)
    }

    /// Report status to the orchestrator.
    pub async fn report_status(&self, update: &StatusUpdate) -> Result<(), WorkerError> {
        let resp = self
            .client
            .post(self.url("status"))
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
            .post(self.url("event"))
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
            .get(self.url("prompt"))
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
            .get(self.url("credentials"))
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
            .post_json("complete", report, "report complete")
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
