//! Unit tests for Anthropic OAuth credential resolution and its
//! interaction with API-key configuration.

use super::super::*;
use crate::config::helpers::ENV_MUTEX;
use crate::settings::Settings;
use crate::testing::credentials::*;

/// Clear all Anthropic-related env vars.
fn clear_anthropic_env() {
    // SAFETY: Only called under ENV_MUTEX in tests.
    unsafe {
        std::env::remove_var("LLM_BACKEND");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("ANTHROPIC_OAUTH_TOKEN");
        std::env::remove_var("ANTHROPIC_MODEL");
        std::env::remove_var("ANTHROPIC_BASE_URL");
    }
}

#[test]
fn anthropic_oauth_token_sets_placeholder_api_key() {
    use secrecy::ExposeSecret;

    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    clear_anthropic_env();
    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::set_var("ANTHROPIC_OAUTH_TOKEN", TEST_ANTHROPIC_OAUTH_TOKEN);
    }

    let settings = Settings {
        llm_backend: Some("anthropic".to_string()),
        ..Default::default()
    };
    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert_eq!(
        provider
            .api_key
            .as_ref()
            .map(|k| k.expose_secret().to_string()),
        Some(OAUTH_PLACEHOLDER.to_string()),
        "api_key should be the OAuth placeholder when only OAuth token is set"
    );
    assert!(
        provider.oauth_token.is_some(),
        "oauth_token should be populated"
    );
    assert_eq!(
        provider.oauth_token.as_ref().unwrap().expose_secret(),
        TEST_ANTHROPIC_OAUTH_TOKEN
    );

    clear_anthropic_env();
}

#[test]
fn anthropic_api_key_takes_priority_over_oauth() {
    use secrecy::ExposeSecret;

    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    clear_anthropic_env();
    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::set_var("ANTHROPIC_API_KEY", TEST_ANTHROPIC_API_KEY);
        std::env::set_var("ANTHROPIC_OAUTH_TOKEN", TEST_ANTHROPIC_OAUTH_TOKEN);
    }

    let settings = Settings {
        llm_backend: Some("anthropic".to_string()),
        ..Default::default()
    };
    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert_eq!(
        provider
            .api_key
            .as_ref()
            .map(|k| k.expose_secret().to_string()),
        Some(TEST_ANTHROPIC_API_KEY.to_string()),
        "real API key should take priority over OAuth placeholder"
    );
    assert!(
        provider.oauth_token.is_some(),
        "oauth_token should still be populated"
    );

    clear_anthropic_env();
}

#[test]
fn non_anthropic_provider_has_no_oauth_token() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    clear_anthropic_env();
    // SAFETY: Under ENV_MUTEX.
    unsafe {
        std::env::set_var("ANTHROPIC_OAUTH_TOKEN", TEST_ANTHROPIC_OAUTH_TOKEN);
    }

    let settings = Settings {
        llm_backend: Some("openai".to_string()),
        ..Default::default()
    };
    let cfg = LlmConfig::resolve(&settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert!(
        provider.oauth_token.is_none(),
        "non-Anthropic providers should not pick up ANTHROPIC_OAUTH_TOKEN"
    );

    clear_anthropic_env();
}
