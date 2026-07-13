//! Unit tests for model selection precedence: env overrides versus persisted
//! `selected_model` settings for the openai-compatible and ollama backends.

use super::super::*;
use super::clear_openai_compatible_env;
use crate::config::helpers::ENV_MUTEX;
use crate::settings::Settings;

#[test]
fn openai_compatible_uses_selected_model_when_llm_model_unset() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    clear_openai_compatible_env();

    let settings = Settings {
        llm_backend: Some("openai_compatible".to_string()),
        openai_compatible_base_url: Some("https://openrouter.ai/api/v1".to_string()),
        selected_model: Some("openai/gpt-5.1-codex".to_string()),
        ..Default::default()
    };

    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert_eq!(provider.model, "openai/gpt-5.1-codex");
}

#[test]
fn openai_compatible_llm_model_env_overrides_selected_model() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    clear_openai_compatible_env();
    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::set_var("LLM_MODEL", "openai/gpt-5-codex");
    }

    let settings = Settings {
        llm_backend: Some("openai_compatible".to_string()),
        openai_compatible_base_url: Some("https://openrouter.ai/api/v1".to_string()),
        selected_model: Some("openai/gpt-5.1-codex".to_string()),
        ..Default::default()
    };

    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert_eq!(provider.model, "openai/gpt-5-codex");

    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::remove_var("LLM_MODEL");
    }
}

/// Clear all ollama-related env vars.
fn clear_ollama_env() {
    // SAFETY: Only called under ENV_MUTEX in tests.
    unsafe {
        std::env::remove_var("LLM_BACKEND");
        std::env::remove_var("OLLAMA_BASE_URL");
        std::env::remove_var("OLLAMA_MODEL");
    }
}

#[test]
fn ollama_uses_selected_model_when_ollama_model_unset() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    clear_ollama_env();

    let settings = Settings {
        llm_backend: Some("ollama".to_string()),
        selected_model: Some("llama3.2".to_string()),
        ..Default::default()
    };

    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert_eq!(provider.model, "llama3.2");
}

#[test]
fn ollama_model_env_overrides_selected_model() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    clear_ollama_env();
    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::set_var("OLLAMA_MODEL", "mistral:latest");
    }

    let settings = Settings {
        llm_backend: Some("ollama".to_string()),
        selected_model: Some("llama3.2".to_string()),
        ..Default::default()
    };

    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert_eq!(provider.model, "mistral:latest");

    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::remove_var("OLLAMA_MODEL");
    }
}

#[test]
fn openai_compatible_preserves_dotted_model_name() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    clear_openai_compatible_env();

    let settings = Settings {
        llm_backend: Some("openai_compatible".to_string()),
        openai_compatible_base_url: Some("http://localhost:11434/v1".to_string()),
        selected_model: Some("llama3.2".to_string()),
        ..Default::default()
    };

    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert_eq!(
        provider.model, "llama3.2",
        "model name with dot must not be truncated"
    );
}
