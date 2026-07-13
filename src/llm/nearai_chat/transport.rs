//! Authentication and HTTP transport for the NEAR AI chat provider.
//!
//! Resolves Bearer credentials (API key or session token), selects the API
//! base URL, and sends chat-completion requests with session-renewal retry.

use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use crate::llm::error::LlmError;
use crate::llm::nearai_chat::NearAiChatProvider;

pub(super) enum ResolvedBearerCredential {
    ApiKey(String),
    SessionToken(String),
}

impl NearAiChatProvider {
    fn uses_private_near_base_url(base_url: &str) -> bool {
        let base_url = base_url.trim_end_matches('/');
        let base_url = base_url.strip_suffix("/v1").unwrap_or(base_url);
        base_url == "https://private.near.ai"
    }

    async fn current_bearer_credential(&self) -> Option<ResolvedBearerCredential> {
        if let Some(ref api_key) = self.config.api_key {
            return Some(ResolvedBearerCredential::ApiKey(
                api_key.expose_secret().to_string(),
            ));
        }

        if self.session.has_token().await {
            return self.session.get_token().await.ok().map(|token| {
                ResolvedBearerCredential::SessionToken(token.expose_secret().to_string())
            });
        }

        self.session
            .get_api_key()
            .await
            .map(|key| ResolvedBearerCredential::ApiKey(key.expose_secret().to_string()))
    }

    pub(super) async fn uses_api_key_auth(&self) -> bool {
        matches!(
            self.current_bearer_credential().await,
            Some(ResolvedBearerCredential::ApiKey(_))
        )
    }

    pub(super) async fn api_base_url(&self) -> String {
        if self.uses_api_key_auth().await && Self::uses_private_near_base_url(&self.config.base_url)
        {
            "https://cloud-api.near.ai".to_string()
        } else {
            self.config.base_url.clone()
        }
    }

    async fn resolve_current_bearer_token(&self) -> Option<String> {
        self.current_bearer_credential()
            .await
            .map(|credential| match credential {
                ResolvedBearerCredential::ApiKey(value)
                | ResolvedBearerCredential::SessionToken(value) => value,
            })
    }

    /// Resolve the Bearer token for the current auth mode.
    ///
    /// Priority order:
    /// 1. `config.api_key` (set at construction from env/config)
    /// 2. Session token (OAuth flow)
    /// 3. Session-managed API key captured during interactive login.
    pub(super) async fn resolve_bearer_token(&self) -> Result<String, LlmError> {
        if let Some(token) = self.resolve_current_bearer_token().await {
            return Ok(token);
        }

        // No token yet, trigger interactive login
        self.session.ensure_authenticated().await?;

        if let Some(token) = self.resolve_current_bearer_token().await {
            return Ok(token);
        }

        Err(LlmError::AuthFailed {
            provider: "nearai".to_string(),
        })
    }

    /// Send a single request to the chat completions API.
    ///
    /// For session token auth, handles 401 by calling `session.handle_auth_failure()`
    /// and retrying once.
    ///
    /// Does not retry on other errors — retries are handled by the external
    /// `RetryProvider` wrapper in the composition chain.
    pub(super) async fn send_request<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        body: &T,
    ) -> Result<R, LlmError> {
        match self.send_request_inner(body).await {
            Ok(result) => Ok(result),
            Err(LlmError::SessionExpired { .. }) if !self.uses_api_key_auth().await => {
                // Session expired, attempt renewal and retry once
                self.session.handle_auth_failure().await?;
                self.send_request_inner(body).await
            }
            Err(e) => Err(e),
        }
    }

    /// Log the serialized request body at DEBUG level.
    ///
    /// Checks the level first so serialization is skipped when DEBUG
    /// logging is disabled.
    fn debug_log_request_body<T: Serialize>(body: &T) {
        if !tracing::enabled!(tracing::Level::DEBUG) {
            return;
        }
        if let Ok(json) = serde_json::to_string(body) {
            tracing::debug!("NEAR AI Chat request body: {}", json);
        }
    }

    /// Inner request implementation (single attempt).
    async fn send_request_inner<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        body: &T,
    ) -> Result<R, LlmError> {
        let token = self.resolve_bearer_token().await?;
        let url = Self::api_url_for_base(&self.api_base_url().await, "chat/completions");

        tracing::debug!("Sending request to NEAR AI Chat: {}", url);
        Self::debug_log_request_body(body);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed {
                provider: "nearai_chat".to_string(),
                reason: e.to_string(),
            })?;

        let status = response.status();
        // Extract Retry-After header before consuming the response body.
        // Supports both delay-seconds (RFC 7231 §7.1.3) and HTTP-date formats.
        let retry_after_header = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                // Try delay-seconds first (most common from API providers)
                if let Ok(secs) = v.trim().parse::<u64>() {
                    return Some(std::time::Duration::from_secs(secs));
                }
                // Try HTTP-date (e.g. "Mon, 02 Mar 2026 18:00:00 GMT")
                if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(v.trim()) {
                    let now = chrono::Utc::now();
                    let delta = dt.signed_duration_since(now);
                    // Use max(0) so past/present dates yield Duration::ZERO
                    // rather than None (which would cause an immediate retry).
                    return Some(std::time::Duration::from_secs(
                        delta.num_seconds().max(0) as u64
                    ));
                }
                None
            });
        let response_text = response.text().await.map_err(|e| LlmError::RequestFailed {
            provider: "nearai_chat".to_string(),
            reason: format!("Failed to read response body: {}", e),
        })?;

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("NEAR AI Chat response status: {}", status);
        }

        // Log response body only at TRACE level to avoid exposing sensitive content
        // (user-generated data, tool outputs, leaked secrets) in DEBUG logs
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!("NEAR AI Chat response body: {}", response_text);
        }

        if !status.is_success() {
            let status_code = status.as_u16();

            if status_code == 401 {
                // For session token auth, distinguish session expired from plain auth failure
                if !self.uses_api_key_auth().await {
                    let lower = response_text.to_lowercase();
                    let is_session_expired = lower.contains("session")
                        && (lower.contains("expired") || lower.contains("invalid"));
                    if is_session_expired {
                        return Err(LlmError::SessionExpired {
                            provider: "nearai_chat".to_string(),
                        });
                    }
                }
                return Err(LlmError::AuthFailed {
                    provider: "nearai_chat".to_string(),
                });
            }

            if status_code == 429 {
                return Err(LlmError::RateLimited {
                    provider: "nearai_chat".to_string(),
                    retry_after: retry_after_header,
                });
            }

            let truncated = crate::agent::truncate_for_preview(&response_text, 512);
            return Err(LlmError::RequestFailed {
                provider: "nearai_chat".to_string(),
                reason: format!("HTTP {}: {}", status, truncated),
            });
        }

        serde_json::from_str(&response_text).map_err(|e| {
            let truncated = crate::agent::truncate_for_preview(&response_text, 512);
            LlmError::InvalidResponse {
                provider: "nearai_chat".to_string(),
                reason: format!("JSON parse error: {}. Raw: {}", e, truncated),
            }
        })
    }
}
