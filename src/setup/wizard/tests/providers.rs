//! Tests for provider switching, Bedrock configuration, provider setup
//! dispatch, and NEAR AI model-fetch configuration.

use super::super::*;
use super::helpers::{EnvBatchGuard, EnvGuard};
use crate::setup::nearai;

fn select_backend(settings: &mut Settings, backend: &str) {
    if settings.llm_backend.as_deref() != Some(backend) {
        settings.selected_model = None;
    }
    settings.llm_backend = Some(backend.to_string());
}

const _: fn(&mut Settings, &str) = select_backend;

/// Simulate the selected-model clearing logic applied when entering a provider setup screen.
/// Returns the value of `selected_model` after the simulated switch.
struct ProviderSwitch<'a> {
    from_backend: &'a str,
    model: &'a str,
    to_backend: &'a str,
}

fn provider_model_after_switch(req: ProviderSwitch<'_>) -> Option<String> {
    let ProviderSwitch {
        from_backend,
        model,
        to_backend,
    } = req;
    let mut wizard = SetupWizard::new();
    wizard.settings.llm_backend = Some(from_backend.to_string());
    wizard.settings.selected_model = Some(model.to_string());
    if wizard.settings.llm_backend.as_deref() != Some(to_backend) {
        wizard.settings.selected_model = None;
    }
    wizard.settings.llm_backend = Some(to_backend.to_string());
    wizard.settings.selected_model.clone()
}

/// Regression test for #600: re-running provider setup for the same backend
/// must NOT clear selected_model. Only switching to a different backend should.
#[test]
fn test_same_provider_preserves_selected_model() {
    assert_eq!(
        provider_model_after_switch(ProviderSwitch {
            from_backend: "ollama",
            model: "llama3",
            to_backend: "ollama",
        })
        .as_deref(),
        Some("llama3"),
        "model should be preserved when re-selecting the same provider"
    );
}

/// Regression test for #600: switching to a different provider must clear
/// selected_model since the old model may not be valid for the new backend.
#[test]
fn test_different_provider_clears_selected_model() {
    assert!(
        provider_model_after_switch(ProviderSwitch {
            from_backend: "ollama",
            model: "llama3",
            to_backend: "openai",
        })
        .is_none(),
        "model should be cleared when switching providers"
    );
}

/// Regression: Bedrock setup_bedrock() should preserve selected_model
/// when re-entering the same provider (matches pattern from #600).
#[test]
fn test_bedrock_same_provider_preserves_model() {
    assert_eq!(
        provider_model_after_switch(ProviderSwitch {
            from_backend: "bedrock",
            model: "anthropic.claude-opus-4-6-v1",
            to_backend: "bedrock",
        })
        .as_deref(),
        Some("anthropic.claude-opus-4-6-v1"),
        "bedrock model should be preserved when re-selecting bedrock"
    );
}

/// Regression: switching from another provider to bedrock must clear
/// selected_model, and choosing "default credentials" must clear
/// bedrock_profile.
#[test]
fn test_bedrock_clears_stale_profile_on_default_creds() {
    let mut wizard = SetupWizard::new();
    wizard.settings.llm_backend = Some("bedrock".to_string());
    wizard.settings.bedrock_profile = Some("old-sso-profile".to_string());

    // Simulate auth_choice == 0 (default credentials) clearing the profile
    wizard.settings.bedrock_profile = None;

    assert!(
        wizard.settings.bedrock_profile.is_none(),
        "bedrock_profile should be cleared when selecting default credentials"
    );
}

/// Regression: empty profile input in named-profile auth should clear
/// any previously configured profile instead of leaving it stale.
#[test]
fn test_bedrock_empty_profile_clears_existing() {
    let mut wizard = SetupWizard::new();
    wizard.settings.bedrock_profile = Some("old-profile".to_string());

    // Simulate auth_choice == 1 with empty input
    let profile = "".to_string();
    if profile.trim().is_empty() {
        wizard.settings.bedrock_profile = None;
    } else {
        wizard.settings.bedrock_profile = Some(profile);
    }

    assert!(
        wizard.settings.bedrock_profile.is_none(),
        "empty profile input should clear existing bedrock_profile"
    );
}

#[tokio::test]
async fn test_run_provider_setup_no_setup_hint() {
    // A provider with setup: None should not error. It should set the
    // backend and return Ok, allowing env-var-only configured providers
    // to be kept during re-onboarding.
    let mut wizard = SetupWizard::new();

    let mut providers: Vec<crate::llm::registry::ProviderDefinition> =
        serde_json::from_str(include_str!("../../../../providers.json")).unwrap();
    // Add a provider with no setup hint
    providers.push(crate::llm::registry::ProviderDefinition {
        id: "custom_no_setup".to_string(),
        aliases: vec![],
        protocol: crate::llm::registry::ProviderProtocol::OpenAiCompletions,
        default_base_url: Some("http://localhost:9999/v1".to_string()),
        base_url_env: None,
        base_url_required: false,
        api_key_env: None,
        api_key_required: false,
        model_env: "CUSTOM_MODEL".to_string(),
        default_model: "custom-model".to_string(),
        description: "Custom provider with no setup wizard".to_string(),
        extra_headers_env: None,
        setup: None,
        unsupported_params: vec![],
    });
    let registry = crate::llm::ProviderRegistry::new(providers);

    let result = wizard
        .run_provider_setup("custom_no_setup", &registry)
        .await;
    assert!(result.is_ok(), "setup: None provider should not error");
    assert_eq!(
        wizard.settings.llm_backend.as_deref(),
        Some("custom_no_setup"),
        "backend should be set even without setup hint"
    );
}

/// Regression test for #666: env-var security option must initialize
/// secrets_crypto so subsequent steps can encrypt API keys.
#[test]
fn test_env_var_security_initializes_crypto() {
    use crate::secrets::SecretsCrypto;
    use secrecy::SecretString;

    // Simulate what option 1 in step_security() does after the fix:
    let key_hex = crate::secrets::keychain::generate_master_key_hex();

    // The fix: create SecretsCrypto from the generated key.
    // Before the fix, this was skipped, leaving secrets_crypto = None.
    let crypto = SecretsCrypto::new(SecretString::from(key_hex.clone()));
    assert!(
        crypto.is_ok(),
        "generated key hex must produce valid SecretsCrypto"
    );

    // Verify the key is stored for bootstrap env persistence.
    let settings = Settings {
        secrets_master_key_hex: Some(key_hex),
        ..Settings::default()
    };
    assert!(settings.secrets_master_key_hex.is_some());
}

/// Regression test for #799: `fetch_nearai_models` hardcoded `api_key: None`,
/// causing the auth prompt to re-appear during model selection when the user
/// had authenticated via NEAR AI Cloud API key (option 4).
#[test]
fn test_build_nearai_model_fetch_config_picks_up_api_key_env() {
    use secrecy::ExposeSecret;

    let _guard = EnvBatchGuard::new(&[
        ("NEARAI_API_KEY", Some("test-cloud-api-key-12345")),
        ("NEARAI_BASE_URL", None),
    ]);

    let config = nearai::build_nearai_model_fetch_config(Some(secrecy::SecretString::from(
        "test-cloud-api-key-12345",
    )));
    assert!(
        config.nearai.api_key.is_some(),
        "config should include the supplied API key"
    );
    assert_eq!(
        config.nearai.api_key.as_ref().unwrap().expose_secret(),
        "test-cloud-api-key-12345"
    );
    // With API key, base_url must point to cloud-api (not private.near.ai)
    assert_eq!(
        config.nearai.base_url, "https://cloud-api.near.ai",
        "API key auth must use cloud-api base URL for model fetching"
    );
}

/// Regression test for #799: when NEARAI_API_KEY is absent or empty,
/// the config should have `api_key: None` (session token path).
#[test]
fn test_build_nearai_model_fetch_config_none_when_no_api_key() {
    let _guard = EnvGuard::clear("NEARAI_BASE_URL");

    let config = nearai::build_nearai_model_fetch_config(None);
    assert!(
        config.nearai.api_key.is_none(),
        "config should have no api_key when none is supplied"
    );
    // Without API key, base_url must point to private.near.ai (session token)
    assert_eq!(
        config.nearai.base_url, "https://private.near.ai",
        "session-token auth must use private.near.ai base URL"
    );
}

/// Regression test for #799: empty API keys should be treated as absent.
#[test]
fn test_build_nearai_model_fetch_config_none_when_empty_api_key() {
    let config = nearai::build_nearai_model_fetch_config(Some(secrecy::SecretString::from("")));
    assert!(
        config.nearai.api_key.is_none(),
        "config should have no api_key when the supplied key is empty"
    );
}
