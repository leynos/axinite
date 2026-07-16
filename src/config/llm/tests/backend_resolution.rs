//! Unit tests for backend name resolution: registry providers, aliases,
//! fallbacks, base URL priority, and request timeout configuration.

use super::super::*;
use super::clear_openai_compatible_env;
use crate::config::helpers::ENV_MUTEX;
use crate::llm::ProviderProtocol;
use crate::settings::Settings;
use crate::testing::credentials::*;

#[test]
fn registry_provider_resolves_groq() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::remove_var("LLM_BACKEND");
        std::env::remove_var("GROQ_API_KEY");
        std::env::remove_var("GROQ_MODEL");
    }

    let settings = Settings {
        llm_backend: Some("groq".to_string()),
        selected_model: Some("llama-3.3-70b-versatile".to_string()),
        ..Default::default()
    };

    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    assert_eq!(cfg.backend, "groq");
    let provider = cfg.provider.expect("provider config should be present");
    assert_eq!(provider.provider_id, "groq");
    assert_eq!(provider.model, "llama-3.3-70b-versatile");
    assert_eq!(provider.base_url, "https://api.groq.com/openai/v1");
    assert_eq!(provider.protocol, ProviderProtocol::OpenAiCompletions);
}

#[test]
fn registry_provider_resolves_tinfoil() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::remove_var("LLM_BACKEND");
        std::env::remove_var("TINFOIL_API_KEY");
        std::env::remove_var("TINFOIL_MODEL");
    }

    let settings = Settings {
        llm_backend: Some("tinfoil".to_string()),
        ..Default::default()
    };

    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    assert_eq!(cfg.backend, "tinfoil");
    let provider = cfg.provider.expect("provider config should be present");
    assert_eq!(provider.base_url, "https://inference.tinfoil.sh/v1");
    assert_eq!(provider.model, "kimi-k2-5");
    assert!(
        provider
            .unsupported_params
            .contains(&"temperature".to_string()),
        "tinfoil should propagate unsupported_params from registry"
    );
}

#[test]
fn nearai_backend_has_no_registry_provider() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::remove_var("LLM_BACKEND");
    }

    let settings = Settings::default();
    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    assert_eq!(cfg.backend, "nearai");
    assert!(cfg.provider.is_none());
}

#[test]
fn backend_alias_normalized_to_canonical_id() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    clear_openai_compatible_env();
    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::set_var("LLM_BACKEND", "open_ai");
        std::env::set_var("OPENAI_API_KEY", TEST_API_KEY);
    }

    let settings = Settings::default();
    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    assert_eq!(
        cfg.backend, "openai",
        "alias 'open_ai' should be normalized to canonical 'openai'"
    );
    let provider = cfg.provider.expect("should have provider config");
    assert_eq!(provider.provider_id, "openai");

    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::remove_var("LLM_BACKEND");
        std::env::remove_var("OPENAI_API_KEY");
    }
}

#[test]
fn unknown_backend_falls_back_to_openai_compatible() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    clear_openai_compatible_env();
    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::set_var("LLM_BACKEND", "some_custom_provider");
        std::env::set_var("LLM_BASE_URL", "http://localhost:8080/v1");
    }

    let settings = Settings::default();
    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    assert_eq!(cfg.backend, "openai_compatible");
    let provider = cfg.provider.expect("should have provider config");
    assert_eq!(provider.provider_id, "openai_compatible");
    assert_eq!(provider.base_url, "http://localhost:8080/v1");

    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::remove_var("LLM_BACKEND");
        std::env::remove_var("LLM_BASE_URL");
    }
}

#[test]
fn unknown_backend_uses_openai_compatible_settings_base_url() {
    let settings = Settings {
        openai_compatible_base_url: Some("http://settings-url/v1".to_string()),
        ..Default::default()
    };
    let ctx = EnvContext::for_testing(
        [(
            "LLM_BACKEND".to_string(),
            "some_custom_provider".to_string(),
        )]
        .into_iter()
        .collect(),
        Default::default(),
    );

    let cfg = LlmConfig::resolve_from(&ctx, &settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("should have provider config");
    assert_eq!(provider.provider_id, "openai_compatible");
    assert_eq!(provider.base_url, "http://settings-url/v1");
}

#[test]
fn nearai_aliases_all_resolve_to_nearai() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");

    for alias in &["nearai", "near_ai", "near"] {
        // SAFETY: Under ENV_MUTEX.
        unsafe {
            std::env::set_var("LLM_BACKEND", alias);
        }
        let settings = Settings::default();
        let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
        assert_eq!(
            cfg.backend, "nearai",
            "alias '{alias}' should resolve to 'nearai'"
        );
        assert!(
            cfg.provider.is_none(),
            "nearai should not have a registry provider"
        );
    }

    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::remove_var("LLM_BACKEND");
    }
}

#[test]
fn base_url_resolution_priority() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    clear_openai_compatible_env();

    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::set_var("LLM_BACKEND", "openai_compatible");
        std::env::set_var("LLM_BASE_URL", "http://env-url/v1");
    }

    let settings = Settings {
        llm_backend: Some("openai_compatible".to_string()),
        openai_compatible_base_url: Some("http://settings-url/v1".to_string()),
        ..Default::default()
    };

    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("should have provider config");
    assert_eq!(
        provider.base_url, "http://env-url/v1",
        "env var should take priority over settings"
    );

    // Now without env var, settings should win over registry default
    unsafe {
        std::env::remove_var("LLM_BASE_URL");
    }

    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("should have provider config");
    assert_eq!(
        provider.base_url, "http://settings-url/v1",
        "settings should take priority over registry default"
    );

    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::remove_var("LLM_BACKEND");
    }
}

#[test]
fn test_request_timeout_defaults_to_120() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::remove_var("LLM_REQUEST_TIMEOUT_SECS");
    }
    let config = LlmConfig::resolve(&Settings::default()).expect("resolve");
    assert_eq!(config.request_timeout_secs, 120);
}

#[test]
fn test_request_timeout_configurable() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::set_var("LLM_REQUEST_TIMEOUT_SECS", "300");
    }
    let config = LlmConfig::resolve(&Settings::default()).expect("resolve");
    assert_eq!(config.request_timeout_secs, 300);
    // SAFETY: Cleanup
    unsafe {
        std::env::remove_var("LLM_REQUEST_TIMEOUT_SECS");
    }
}
