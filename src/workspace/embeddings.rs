//! Embedding providers for semantic search.
//!
//! Embeddings convert text into dense vectors that capture semantic meaning.
//! Similar concepts have similar vectors, enabling semantic search.

use core::future::Future;
use core::pin::Pin;

mod mock;
mod nearai;
mod ollama;
mod openai;

pub use mock::MockEmbeddings;
pub use nearai::NearAiEmbeddings;
pub use ollama::OllamaEmbeddings;
pub use openai::OpenAiEmbeddings;

/// Error type for embedding operations.
#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("HTTP request failed: {0}")]
    HttpError(String),

    #[error("Invalid response: {0}")]
    InvalidResponse(String),

    #[error("Rate limited, retry after {retry_after:?}")]
    RateLimited {
        retry_after: Option<std::time::Duration>,
    },

    #[error("Authentication failed")]
    AuthFailed,

    #[error("Text too long: {length} > {max}")]
    TextTooLong { length: usize, max: usize },
}

impl From<reqwest::Error> for EmbeddingError {
    fn from(e: reqwest::Error) -> Self {
        EmbeddingError::HttpError(e.to_string())
    }
}

/// Boxed future used at the dyn embedding-provider boundary.
pub type EmbeddingProviderFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for embedding providers.
pub trait EmbeddingProvider: Send + Sync {
    /// Get the embedding dimension.
    fn dimension(&self) -> usize;

    /// Get the model name.
    fn model_name(&self) -> &str;

    /// Maximum input length in characters.
    fn max_input_length(&self) -> usize;

    /// Generate an embedding for a single text.
    fn embed<'a>(
        &'a self,
        text: &'a str,
    ) -> EmbeddingProviderFuture<'a, Result<Vec<f32>, EmbeddingError>>;

    /// Generate embeddings for multiple texts (batched).
    fn embed_batch<'a>(
        &'a self,
        texts: &'a [String],
    ) -> EmbeddingProviderFuture<'a, Result<Vec<Vec<f32>>, EmbeddingError>>;
}

/// Native async sibling trait for concrete embedding-provider implementations.
pub trait NativeEmbeddingProvider: Send + Sync {
    /// Get the embedding dimension.
    fn dimension(&self) -> usize;

    /// Get the model name.
    fn model_name(&self) -> &str;

    /// Maximum input length in characters.
    fn max_input_length(&self) -> usize;

    /// Generate an embedding for a single text.
    fn embed<'a>(
        &'a self,
        text: &'a str,
    ) -> impl Future<Output = Result<Vec<f32>, EmbeddingError>> + Send + 'a;

    /// Generate embeddings for multiple texts (batched).
    ///
    /// Default implementation calls embed() for each text.
    fn embed_batch<'a>(
        &'a self,
        texts: &'a [String],
    ) -> impl Future<Output = Result<Vec<Vec<f32>>, EmbeddingError>> + Send + 'a {
        async move {
            let mut embeddings = Vec::with_capacity(texts.len());
            for text in texts {
                embeddings.push(self.embed(text).await?);
            }
            Ok(embeddings)
        }
    }
}

impl<T> EmbeddingProvider for T
where
    T: NativeEmbeddingProvider + Send + Sync,
{
    fn dimension(&self) -> usize {
        NativeEmbeddingProvider::dimension(self)
    }

    fn model_name(&self) -> &str {
        NativeEmbeddingProvider::model_name(self)
    }

    fn max_input_length(&self) -> usize {
        NativeEmbeddingProvider::max_input_length(self)
    }

    fn embed<'a>(
        &'a self,
        text: &'a str,
    ) -> EmbeddingProviderFuture<'a, Result<Vec<f32>, EmbeddingError>> {
        Box::pin(NativeEmbeddingProvider::embed(self, text))
    }

    fn embed_batch<'a>(
        &'a self,
        texts: &'a [String],
    ) -> EmbeddingProviderFuture<'a, Result<Vec<Vec<f32>>, EmbeddingError>> {
        Box::pin(NativeEmbeddingProvider::embed_batch(self, texts))
    }
}

#[cfg(test)]
mod tests;
