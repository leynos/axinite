//! Unit tests for API key redaction in imported config debug output.

use super::*;

#[test]
fn test_llm_config_debug_redacts_api_key() {
    let config = OpenClawLlmConfig {
        provider: Some("openai".to_string()),
        model: Some("gpt-4".to_string()),
        api_key: Some(SecretString::new("sk-secret-key-12345".into())),
        base_url: Some("https://api.openai.com".to_string()),
    };

    let debug_output = format!("{:?}", config);

    // Verify the actual API key is never exposed in debug output
    assert!(!debug_output.contains("sk-secret-key-12345"));
    // Verify the redaction marker is present
    assert!(debug_output.contains("***REDACTED***"));
}

#[test]
fn test_embeddings_config_debug_redacts_api_key() {
    let config = OpenClawEmbeddingsConfig {
        model: Some("text-embedding-3-large".to_string()),
        api_key: Some(SecretString::new("sk-embed-secret-67890".into())),
        provider: Some("openai".to_string()),
    };

    let debug_output = format!("{:?}", config);

    // Verify the actual API key is never exposed in debug output
    assert!(!debug_output.contains("sk-embed-secret-67890"));
    // Verify the redaction marker is present
    assert!(debug_output.contains("***REDACTED***"));
}

#[test]
fn test_llm_config_without_api_key() {
    let config = OpenClawLlmConfig {
        provider: Some("openai".to_string()),
        model: Some("gpt-4".to_string()),
        api_key: None,
        base_url: None,
    };

    let debug_output = format!("{:?}", config);

    // Should show None for missing API key
    assert!(debug_output.contains("api_key: None"));
}
