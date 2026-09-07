//! Unit tests for model selection precedence: env overrides versus persisted
//! `selected_model` settings for the openai-compatible and ollama backends.

use super::super::*;
use crate::settings::Settings;

#[test]
fn openai_compatible_uses_selected_model_when_llm_model_unset() {
    let settings = Settings {
        llm_backend: Some("openai_compatible".to_string()),
        openai_compatible_base_url: Some("https://openrouter.ai/api/v1".to_string()),
        selected_model: Some("openai/gpt-5.1-codex".to_string()),
        ..Default::default()
    };

    let cfg =
        LlmConfig::resolve_from(&EnvContext::default(), &settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert_eq!(provider.model, "openai/gpt-5.1-codex");
}

#[test]
fn openai_compatible_llm_model_env_overrides_selected_model() {
    let ctx = EnvContext::default().with_env("LLM_MODEL", "openai/gpt-5-codex");

    let settings = Settings {
        llm_backend: Some("openai_compatible".to_string()),
        openai_compatible_base_url: Some("https://openrouter.ai/api/v1".to_string()),
        selected_model: Some("openai/gpt-5.1-codex".to_string()),
        ..Default::default()
    };

    let cfg = LlmConfig::resolve_from(&ctx, &settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert_eq!(provider.model, "openai/gpt-5-codex");
}

#[test]
fn ollama_uses_selected_model_when_ollama_model_unset() {
    let settings = Settings {
        llm_backend: Some("ollama".to_string()),
        selected_model: Some("llama3.2".to_string()),
        ..Default::default()
    };

    let cfg =
        LlmConfig::resolve_from(&EnvContext::default(), &settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert_eq!(provider.model, "llama3.2");
}

#[test]
fn ollama_model_env_overrides_selected_model() {
    let ctx = EnvContext::default().with_env("OLLAMA_MODEL", "mistral:latest");

    let settings = Settings {
        llm_backend: Some("ollama".to_string()),
        selected_model: Some("llama3.2".to_string()),
        ..Default::default()
    };

    let cfg = LlmConfig::resolve_from(&ctx, &settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert_eq!(provider.model, "mistral:latest");
}

#[test]
fn openai_compatible_preserves_dotted_model_name() {
    let settings = Settings {
        llm_backend: Some("openai_compatible".to_string()),
        openai_compatible_base_url: Some("http://localhost:11434/v1".to_string()),
        selected_model: Some("llama3.2".to_string()),
        ..Default::default()
    };

    let cfg =
        LlmConfig::resolve_from(&EnvContext::default(), &settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert_eq!(
        provider.model, "llama3.2",
        "model name with dot must not be truncated"
    );
}
