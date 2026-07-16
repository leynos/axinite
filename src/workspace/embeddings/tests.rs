//! Unit tests for workspace embedding providers.

use super::*;

#[tokio::test]
async fn test_mock_embeddings() {
    let provider = MockEmbeddings::new(128);

    let embedding = NativeEmbeddingProvider::embed(&provider, "hello world")
        .await
        .unwrap();
    assert_eq!(embedding.len(), 128);

    // Check normalization (should be unit vector)
    let magnitude: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((magnitude - 1.0).abs() < 0.001);
}

#[tokio::test]
async fn test_mock_embeddings_deterministic() {
    let provider = MockEmbeddings::new(64);

    let emb1 = NativeEmbeddingProvider::embed(&provider, "test")
        .await
        .unwrap();
    let emb2 = NativeEmbeddingProvider::embed(&provider, "test")
        .await
        .unwrap();

    // Same input should produce same embedding
    assert_eq!(emb1, emb2);
}

#[tokio::test]
async fn test_mock_embeddings_batch() {
    let provider = MockEmbeddings::new(64);

    let texts = vec!["hello".to_string(), "world".to_string()];
    let embeddings = NativeEmbeddingProvider::embed_batch(&provider, &texts)
        .await
        .unwrap();

    assert_eq!(embeddings.len(), 2);
    assert_eq!(embeddings[0].len(), 64);
    assert_eq!(embeddings[1].len(), 64);

    // Different texts should produce different embeddings
    assert_ne!(embeddings[0], embeddings[1]);
}

#[test]
fn test_openai_embeddings_config() {
    let provider = OpenAiEmbeddings::new("test-key");
    assert_eq!(NativeEmbeddingProvider::dimension(&provider), 1536);
    assert_eq!(
        NativeEmbeddingProvider::model_name(&provider),
        "text-embedding-3-small"
    );

    let provider = OpenAiEmbeddings::large("test-key");
    assert_eq!(NativeEmbeddingProvider::dimension(&provider), 3072);
    assert_eq!(
        NativeEmbeddingProvider::model_name(&provider),
        "text-embedding-3-large"
    );
}
