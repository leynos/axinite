//! NEAR AI embedding provider using session-based authentication.

use serde::{Deserialize, Serialize};

use super::{EmbeddingError, NativeEmbeddingProvider};

/// NEAR AI embedding provider using the NEAR AI API.
///
/// Uses the same session-based auth as the LLM provider.
pub struct NearAiEmbeddings {
    client: reqwest::Client,
    base_url: String,
    session: std::sync::Arc<crate::llm::SessionManager>,
    model: String,
    dimension: usize,
}

impl NearAiEmbeddings {
    /// Create a new NEAR AI embedding provider.
    ///
    /// Uses the same session manager as the LLM provider for auth.
    pub fn new(
        base_url: impl Into<String>,
        session: std::sync::Arc<crate::llm::SessionManager>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.into(),
            session,
            model: "text-embedding-3-small".to_string(),
            dimension: 1536,
        }
    }

    /// Use a specific model.
    pub fn with_model(mut self, model: impl Into<String>, dimension: usize) -> Self {
        self.model = model.into();
        self.dimension = dimension;
        self
    }
}

#[derive(Debug, Serialize)]
struct NearAiEmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Debug, Deserialize)]
struct NearAiEmbeddingResponse {
    data: Vec<NearAiEmbeddingData>,
}

#[derive(Debug, Deserialize)]
struct NearAiEmbeddingData {
    embedding: Vec<f32>,
}

impl NativeEmbeddingProvider for NearAiEmbeddings {
    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn max_input_length(&self) -> usize {
        32_000
    }

    async fn embed<'a>(&'a self, text: &'a str) -> Result<Vec<f32>, EmbeddingError> {
        if text.len() > NativeEmbeddingProvider::max_input_length(self) {
            return Err(EmbeddingError::TextTooLong {
                length: text.len(),
                max: NativeEmbeddingProvider::max_input_length(self),
            });
        }

        let embeddings = NativeEmbeddingProvider::embed_batch(self, &[text.to_string()]).await?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| EmbeddingError::InvalidResponse("No embedding returned".to_string()))
    }

    async fn embed_batch<'a>(
        &'a self,
        texts: &'a [String],
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        use secrecy::ExposeSecret;

        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let request = NearAiEmbeddingRequest {
            model: &self.model,
            input: texts,
        };

        let token = self
            .session
            .get_token()
            .await
            .map_err(|_| EmbeddingError::AuthFailed)?;

        let url = format!("{}/v1/embeddings", self.base_url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token.expose_secret()))
            .json(&request)
            .send()
            .await?;

        let status = response.status();

        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(EmbeddingError::AuthFailed);
        }

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .map(std::time::Duration::from_secs);
            return Err(EmbeddingError::RateLimited { retry_after });
        }

        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(EmbeddingError::HttpError(format!(
                "Status {}: {}",
                status, error_text
            )));
        }

        let result: NearAiEmbeddingResponse = response.json().await.map_err(|e| {
            EmbeddingError::InvalidResponse(format!("Failed to parse response: {}", e))
        })?;

        Ok(result.data.into_iter().map(|d| d.embedding).collect())
    }
}
