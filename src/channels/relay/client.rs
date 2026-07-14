//! HTTP client for the channel-relay service.
//!
//! Wraps reqwest for all channel-relay API calls: OAuth initiation,
//! SSE streaming, token renewal, and Slack API proxy.

use secrecy::{ExposeSecret, SecretString};
use tokio::sync::mpsc;

mod events;
mod sse;

#[cfg(test)]
mod tests;

pub use events::{ChannelEvent, Connection, event_types};
pub use sse::ChannelEventStream;

use sse::parse_sse_stream;

/// HTTP client for the channel-relay service.
#[derive(Clone)]
pub struct RelayClient {
    http: reqwest::Client,
    base_url: String,
    api_key: SecretString,
}

impl RelayClient {
    /// Create a new relay client.
    pub fn new(
        base_url: String,
        api_key: SecretString,
        request_timeout_secs: u64,
    ) -> Result<Self, RelayError> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(request_timeout_secs))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| RelayError::Network(format!("Failed to build HTTP client: {e}")))?;

        Ok(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
        })
    }

    /// Initiate Slack OAuth flow via channel-relay.
    ///
    /// Calls `GET /oauth/slack/auth` with `redirect(Policy::none())` and
    /// returns the `Location` header (Slack OAuth URL) without following it.
    pub async fn initiate_oauth(
        &self,
        instance_id: &str,
        user_id: &str,
        callback_url: &str,
    ) -> Result<String, RelayError> {
        let resp = self
            .http
            .get(format!("{}/oauth/slack/auth", self.base_url))
            .header("X-API-Key", self.api_key.expose_secret())
            .query(&[
                ("instance_id", instance_id),
                ("user_id", user_id),
                ("callback", callback_url),
            ])
            .send()
            .await
            .map_err(|e| RelayError::Network(e.to_string()))?;

        let status = resp.status();
        if status.is_redirection() {
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
                .ok_or_else(|| {
                    RelayError::Protocol("Redirect response missing Location header".to_string())
                })?;
            Ok(location)
        } else if status.is_success() {
            // Some relay implementations return the URL in JSON body instead
            let body: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| RelayError::Protocol(e.to_string()))?;
            body.get("auth_url")
                .or_else(|| body.get("url"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| RelayError::Protocol("Response missing auth_url field".to_string()))
        } else {
            let body = resp.text().await.unwrap_or_default();
            Err(RelayError::Api {
                status: status.as_u16(),
                message: body,
            })
        }
    }

    /// Connect to the SSE event stream.
    ///
    /// Returns a stream of parsed `ChannelEvent`s and the `JoinHandle` of the
    /// background SSE parser task. The caller is responsible for reconnection
    /// logic on stream end/error and for aborting the handle on shutdown.
    pub async fn connect_stream(
        &self,
        stream_token: &str,
        stream_timeout_secs: u64,
    ) -> Result<(ChannelEventStream, tokio::task::JoinHandle<()>), RelayError> {
        let resp = self
            .http
            .get(format!("{}/stream", self.base_url))
            .query(&[("token", stream_token)])
            .timeout(std::time::Duration::from_secs(stream_timeout_secs))
            .send()
            .await
            .map_err(|e| RelayError::Network(e.to_string()))?;

        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(RelayError::TokenExpired);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(RelayError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        // Spawn a background task that reads the SSE stream and sends parsed events
        let (tx, rx) = mpsc::channel(64);
        let byte_stream = resp.bytes_stream();
        let handle = tokio::spawn(parse_sse_stream(byte_stream, tx));

        Ok((ChannelEventStream { rx }, handle))
    }

    /// Renew an expired stream token.
    ///
    /// Calls `POST /stream/renew` with API key auth, returns a new stream token.
    pub async fn renew_token(
        &self,
        instance_id: &str,
        user_id: &str,
    ) -> Result<String, RelayError> {
        let resp = self
            .http
            .post(format!("{}/stream/renew", self.base_url))
            .header("X-API-Key", self.api_key.expose_secret())
            .json(&serde_json::json!({
                "instance_id": instance_id,
                "user_id": user_id,
            }))
            .send()
            .await
            .map_err(|e| RelayError::Network(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(RelayError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| RelayError::Protocol(e.to_string()))?;
        body.get("stream_token")
            .or_else(|| body.get("token"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| RelayError::Protocol("Response missing stream_token field".to_string()))
    }

    /// Proxy an API call through channel-relay for any provider.
    ///
    /// Calls `POST /proxy/{provider}/{method}?team_id=X&instance_id=Y` with the given JSON body.
    pub async fn proxy_provider(
        &self,
        provider: &str,
        team_id: &str,
        method: &str,
        body: serde_json::Value,
        instance_id: Option<&str>,
    ) -> Result<serde_json::Value, RelayError> {
        let mut query: Vec<(&str, &str)> = vec![("team_id", team_id)];
        if let Some(iid) = instance_id {
            query.push(("instance_id", iid));
        }
        let resp = self
            .http
            .post(format!("{}/proxy/{}/{}", self.base_url, provider, method))
            .header("X-API-Key", self.api_key.expose_secret())
            .query(&query)
            .json(&body)
            .send()
            .await
            .map_err(|e| RelayError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(RelayError::Api {
                status,
                message: body,
            });
        }

        resp.json()
            .await
            .map_err(|e| RelayError::Protocol(e.to_string()))
    }

    /// List active connections for an instance.
    pub async fn list_connections(&self, instance_id: &str) -> Result<Vec<Connection>, RelayError> {
        let resp = self
            .http
            .get(format!("{}/connections", self.base_url))
            .header("X-API-Key", self.api_key.expose_secret())
            .query(&[("instance_id", instance_id)])
            .send()
            .await
            .map_err(|e| RelayError::Network(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(RelayError::Api {
                status,
                message: body,
            });
        }

        resp.json()
            .await
            .map_err(|e| RelayError::Protocol(e.to_string()))
    }
}

/// Errors from relay client operations.
#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("Network error: {0}")]
    Network(String),

    #[error("API error (HTTP {status}): {message}")]
    Api { status: u16, message: String },

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Stream token expired")]
    TokenExpired,
}
