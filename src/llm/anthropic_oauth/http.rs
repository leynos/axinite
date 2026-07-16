//! HTTP transport for the Anthropic OAuth provider: endpoint resolution,
//! request dispatch, and 401 token-refresh retry handling.

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use crate::llm::error::LlmError;

use super::{
    ANTHROPIC_API_URL, ANTHROPIC_API_VERSION, ANTHROPIC_OAUTH_BETA, AnthropicOAuthProvider,
    AnthropicRequest,
};

impl AnthropicOAuthProvider {
    pub(super) fn api_url(&self) -> String {
        if let Some(ref base) = self.base_url {
            let base = base.trim_end_matches('/');
            format!("{}/v1/messages", base)
        } else {
            ANTHROPIC_API_URL.to_string()
        }
    }

    pub(super) async fn send_request<R: for<'de> Deserialize<'de>>(
        &self,
        body: &AnthropicRequest,
    ) -> Result<R, LlmError> {
        let url = self.api_url();

        tracing::debug!("Sending request to Anthropic OAuth: {}", url);

        let response = self
            .client
            .post(&url)
            .bearer_auth(self.token.expose_secret())
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("anthropic-beta", ANTHROPIC_OAUTH_BETA)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed {
                provider: "anthropic_oauth".to_string(),
                reason: e.to_string(),
            })?;

        let status = response.status();

        if !status.is_success() {
            // Parse Retry-After header before consuming the body.
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .map(std::time::Duration::from_secs);

            let response_text = response
                .text()
                .await
                .unwrap_or_else(|e| format!("(failed to read error body: {e})"));

            if status.as_u16() == 401 {
                // OAuth tokens from `claude login` expire in ~8-12h. Attempt
                // to re-extract a fresh token from the OS credential store
                // (macOS Keychain / Linux credentials file) before giving up.
                if let Some(fresh) = crate::config::ClaudeCodeConfig::extract_oauth_token() {
                    let fresh_token = SecretString::from(fresh);
                    // Retry once with the refreshed token
                    let retry = self
                        .client
                        .post(&url)
                        .bearer_auth(fresh_token.expose_secret())
                        .header("anthropic-version", ANTHROPIC_API_VERSION)
                        .header("anthropic-beta", ANTHROPIC_OAUTH_BETA)
                        .header("Content-Type", "application/json")
                        .json(body)
                        .send()
                        .await
                        .map_err(|e| LlmError::RequestFailed {
                            provider: "anthropic_oauth".to_string(),
                            reason: e.to_string(),
                        })?;
                    if retry.status().is_success() {
                        let text = retry.text().await.map_err(|e| LlmError::RequestFailed {
                            provider: "anthropic_oauth".to_string(),
                            reason: format!("Failed to read response body: {}", e),
                        })?;
                        return serde_json::from_str(&text).map_err(|e| {
                            let truncated = crate::agent::truncate_for_preview(&text, 512);
                            LlmError::InvalidResponse {
                                provider: "anthropic_oauth".to_string(),
                                reason: format!("JSON parse error: {}. Raw: {}", e, truncated),
                            }
                        });
                    }
                    tracing::warn!(
                        "Anthropic OAuth 401 retry with refreshed token also failed ({})",
                        retry.status()
                    );
                }
                return Err(LlmError::AuthFailed {
                    provider: "anthropic_oauth".to_string(),
                });
            }
            if status.as_u16() == 429 {
                return Err(LlmError::RateLimited {
                    provider: "anthropic_oauth".to_string(),
                    retry_after,
                });
            }
            let truncated = crate::agent::truncate_for_preview(&response_text, 512);
            return Err(LlmError::RequestFailed {
                provider: "anthropic_oauth".to_string(),
                reason: format!("HTTP {}: {}", status, truncated),
            });
        }

        let response_text = response.text().await.map_err(|e| LlmError::RequestFailed {
            provider: "anthropic_oauth".to_string(),
            reason: format!("Failed to read response body: {}", e),
        })?;

        tracing::debug!(
            "Anthropic OAuth response: status={}, bytes={}",
            status,
            response_text.len()
        );

        serde_json::from_str(&response_text).map_err(|e| {
            let truncated = crate::agent::truncate_for_preview(&response_text, 512);
            LlmError::InvalidResponse {
                provider: "anthropic_oauth".to_string(),
                reason: format!("JSON parse error: {}. Raw: {}", e, truncated),
            }
        })
    }
}
