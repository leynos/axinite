//! Unit tests for Anthropic OAuth credential resolution and its
//! interaction with API-key configuration.

use super::super::*;
use crate::settings::Settings;
use crate::testing::credentials::*;

#[test]
fn anthropic_oauth_token_sets_placeholder_api_key() {
    use secrecy::ExposeSecret;

    let ctx = EnvContext::default().with_env("ANTHROPIC_OAUTH_TOKEN", TEST_ANTHROPIC_OAUTH_TOKEN);

    let settings = Settings {
        llm_backend: Some("anthropic".to_string()),
        ..Default::default()
    };
    let cfg = LlmConfig::resolve_from(&ctx, &settings).expect("resolve should succeed");
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
}

#[test]
fn anthropic_api_key_takes_priority_over_oauth() {
    use secrecy::ExposeSecret;

    let ctx = EnvContext::default()
        .with_env("ANTHROPIC_API_KEY", TEST_ANTHROPIC_API_KEY)
        .with_env("ANTHROPIC_OAUTH_TOKEN", TEST_ANTHROPIC_OAUTH_TOKEN);

    let settings = Settings {
        llm_backend: Some("anthropic".to_string()),
        ..Default::default()
    };
    let cfg = LlmConfig::resolve_from(&ctx, &settings).expect("resolve should succeed");
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
}

#[test]
fn non_anthropic_provider_has_no_oauth_token() {
    let ctx = EnvContext::default().with_env("ANTHROPIC_OAUTH_TOKEN", TEST_ANTHROPIC_OAUTH_TOKEN);

    let settings = Settings {
        llm_backend: Some("openai".to_string()),
        ..Default::default()
    };
    let cfg = LlmConfig::resolve_from(&ctx, &settings).expect("resolve should succeed");
    let provider = cfg.provider.expect("provider config should be present");

    assert!(
        provider.oauth_token.is_none(),
        "non-Anthropic providers should not pick up ANTHROPIC_OAUTH_TOKEN"
    );
}
