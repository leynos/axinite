//! Model listing for the NEAR AI chat provider.
//!
//! Fetches available models from the `/v1/models` endpoint, tolerating the
//! several response shapes NEAR AI deployments return.

use serde::Deserialize;

use crate::llm::error::LlmError;
use crate::llm::nearai_chat::{ModelInfo, NearAiChatProvider};

// Flexible model entry parsing -- handle various field names
#[derive(Deserialize)]
struct ModelMetadataInner {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, alias = "modelName", alias = "model_name")]
    model_name: Option<String>,
}

#[derive(Deserialize)]
struct ModelEntry {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, alias = "modelName", alias = "model_name")]
    model_name: Option<String>,
    #[serde(default, alias = "modelId", alias = "model_id")]
    model_id: Option<String>,
    #[serde(default)]
    metadata: Option<ModelMetadataInner>,
}

impl ModelEntry {
    fn get_name(&self) -> Option<String> {
        self.name
            .clone()
            .or_else(|| self.id.clone())
            .or_else(|| self.model.clone())
            .or_else(|| self.model_name.clone())
            .or_else(|| self.model_id.clone())
            .or_else(|| self.metadata.as_ref().and_then(|m| m.name.clone()))
            .or_else(|| self.metadata.as_ref().and_then(|m| m.model_name.clone()))
    }
}

#[derive(Deserialize)]
struct ModelsResponse {
    #[serde(default)]
    models: Option<Vec<ModelEntry>>,
    #[serde(default)]
    data: Option<Vec<ModelEntry>>,
}

/// Convert parsed entries into `ModelInfo`s, dropping nameless entries.
fn entries_to_models(entries: Vec<ModelEntry>) -> Vec<ModelInfo> {
    entries
        .into_iter()
        .filter_map(|e| {
            e.get_name().map(|name| ModelInfo {
                name,
                provider: None,
            })
        })
        .collect()
}

/// Parse a models response body, trying `{models: [...]}`, `{data: [...]}`,
/// and plain-array shapes in turn.
fn parse_models_response(response_text: &str) -> Result<Vec<ModelInfo>, LlmError> {
    // Try {models: [...]} or {data: [...]} format
    if let Ok(resp) = serde_json::from_str::<ModelsResponse>(response_text)
        && let Some(entries) = resp.models.or(resp.data)
    {
        let models = entries_to_models(entries);
        if !models.is_empty() {
            return Ok(models);
        }
    }

    // Try direct array format
    if let Ok(entries) = serde_json::from_str::<Vec<ModelEntry>>(response_text) {
        let models = entries_to_models(entries);
        if !models.is_empty() {
            return Ok(models);
        }
    }

    // Couldn't find model names in response
    Err(LlmError::InvalidResponse {
        provider: "nearai_chat".to_string(),
        reason: format!(
            "No model names found in response: {}",
            &response_text[..response_text.len().min(300)]
        ),
    })
}

impl NearAiChatProvider {
    /// Fetch available models from the NEAR AI API.
    ///
    /// Handles session renewal on 401 (same pattern as `send_request`).
    /// Supports multiple response formats: `{models: [...]}`, `{data: [...]}`, and plain array.
    pub async fn list_models_full(&self) -> Result<Vec<ModelInfo>, LlmError> {
        match self.list_models_inner().await {
            Ok(models) => Ok(models),
            Err(LlmError::SessionExpired { .. }) if !self.uses_api_key_auth().await => {
                self.session.handle_auth_failure().await?;
                self.list_models_inner().await
            }
            Err(e) => Err(e),
        }
    }

    /// Map a non-success HTTP status to the appropriate error: expired
    /// session for 401 under session auth, request failure otherwise.
    async fn models_http_error(
        &self,
        status: reqwest::StatusCode,
        response_text: &str,
    ) -> LlmError {
        if status.as_u16() == 401 && !self.uses_api_key_auth().await {
            return LlmError::SessionExpired {
                provider: "nearai_chat".to_string(),
            };
        }
        let truncated = crate::agent::truncate_for_preview(response_text, 512);
        LlmError::RequestFailed {
            provider: "nearai_chat".to_string(),
            reason: format!("HTTP {}: {}", status, truncated),
        }
    }

    async fn list_models_inner(&self) -> Result<Vec<ModelInfo>, LlmError> {
        let token = self.resolve_bearer_token().await?;
        let url = Self::api_url_for_base(&self.api_base_url().await, "models");

        tracing::debug!("Fetching models from: {}", url);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed {
                provider: "nearai_chat".to_string(),
                reason: format!("Failed to fetch models: {}", e),
            })?;

        let status = response.status();
        let response_text = response.text().await.map_err(|e| LlmError::RequestFailed {
            provider: "nearai_chat".to_string(),
            reason: format!("Failed to read response body: {}", e),
        })?;

        if !status.is_success() {
            return Err(self.models_http_error(status, &response_text).await);
        }

        parse_models_response(&response_text)
    }
}
