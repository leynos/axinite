//! Additional WorkerHttpClient methods for status reporting, events, and credentials.

use crate::error::WorkerError;
use crate::worker::api::{
    COMPLETE_PATH, CREDENTIALS_PATH, CompletionReport, CredentialResponse, EVENT_PATH,
    JobEventPayload, PROMPT_PATH, PromptResponse, STATUS_PATH, StatusUpdate,
};

use super::WorkerHttpClient;

impl WorkerHttpClient {
    /// Report status to the orchestrator.
    pub async fn report_status(&self, update: &StatusUpdate) -> Result<(), WorkerError> {
        let resp = self
            .client
            .post(self.url(STATUS_PATH))
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

    /// Post a job event to the orchestrator.
    ///
    /// Returns `Ok(())` on success, or `WorkerError::ConnectionFailed` if the
    /// request fails or returns a non-success status.
    pub async fn post_event(&self, payload: &JobEventPayload) -> Result<(), WorkerError> {
        let resp = self
            .client
            .post(self.url(EVENT_PATH))
            .bearer_auth(&self.token)
            .json(payload)
            .send()
            .await
            .map_err(|e| WorkerError::ConnectionFailed {
                url: self.url(EVENT_PATH),
                reason: format!("job event POST failed: {}", e),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(WorkerError::OrchestratorRejected {
                job_id: self.job_id,
                reason: format!("job event POST returned {}: {}", status, body),
            });
        }

        Ok(())
    }

    /// Poll the orchestrator for a follow-up prompt.
    ///
    /// Returns `None` if no prompt is available (204 No Content).
    pub async fn poll_prompt(&self) -> Result<Option<PromptResponse>, WorkerError> {
        let resp = self
            .client
            .get(self.url(PROMPT_PATH))
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
            resp.json()
                .await
                .map_err(|e| WorkerError::OrchestratorRejected {
                    job_id: self.job_id,
                    reason: format!("failed to parse prompt response: {}", e),
                })?;

        Ok(Some(prompt))
    }

    /// Fetch credentials granted to this job from the orchestrator.
    ///
    /// Returns an empty vec if no credentials are granted (204 No Content).
    /// Fetched credentials should be handed off to
    /// [`WorkerRuntime::hydrate_credentials`](crate::worker::container::WorkerRuntime::hydrate_credentials),
    /// which stores them in its `extra_env` and injects them into child processes.
    /// Callers should use this runtime hydrate/injection pathway rather than
    /// setting global environment variables directly.
    pub async fn fetch_credentials(&self) -> Result<Vec<CredentialResponse>, WorkerError> {
        let resp = self
            .client
            .get(self.url(CREDENTIALS_PATH))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| WorkerError::ConnectionFailed {
                url: self.orchestrator_url.clone(),
                reason: e.to_string(),
            })?;

        // 204 means no credentials granted, not an error
        if resp.status() == reqwest::StatusCode::NO_CONTENT {
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
            .post_json(COMPLETE_PATH, report, "report complete")
            .await?;
        Ok(())
    }
}
