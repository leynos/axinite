//! Tests for registry lookup and validation of the built-in provider data.

use crate::llm::registry::{ProviderDefinition, ProviderProtocol, ProviderRegistry, SetupHint};

fn builtin_registry() -> ProviderRegistry {
    ProviderRegistry::new(serde_json::from_str(include_str!("../../../../providers.json")).unwrap())
}

fn builtin_providers() -> Vec<ProviderDefinition> {
    serde_json::from_str(include_str!("../../../../providers.json")).unwrap()
}

#[test]
fn test_builtin_registry_loads() {
    let registry = builtin_registry();
    assert!(
        registry.all().len() >= 5,
        "should have at least 5 built-in providers"
    );
}

#[test]
fn test_find_by_id() {
    let registry = builtin_registry();
    let openai = registry.find("openai").expect("openai should exist");
    assert_eq!(openai.id, "openai");
    assert_eq!(openai.protocol, ProviderProtocol::OpenAiCompletions);
}

#[test]
fn test_find_by_alias() {
    let registry = builtin_registry();
    let openai = registry
        .find("open_ai")
        .expect("alias open_ai should resolve");
    assert_eq!(openai.id, "openai");
}

#[test]
fn test_find_case_insensitive() {
    let registry = builtin_registry();
    assert!(registry.find("OpenAI").is_some());
    assert!(registry.find("GROQ").is_some());
    assert!(registry.find("Tinfoil").is_some());
}

#[test]
fn test_find_unknown_returns_none() {
    let registry = builtin_registry();
    assert!(registry.find("nonexistent_provider").is_none());
}

#[test]
fn test_model_env_var_nearai() {
    let registry = builtin_registry();
    assert_eq!(registry.model_env_var("nearai"), "NEARAI_MODEL");
    assert_eq!(registry.model_env_var("near_ai"), "NEARAI_MODEL");
}

#[test]
fn test_model_env_var_registry_provider() {
    let registry = builtin_registry();
    assert_eq!(registry.model_env_var("groq"), "GROQ_MODEL");
    assert_eq!(registry.model_env_var("tinfoil"), "TINFOIL_MODEL");
    assert_eq!(registry.model_env_var("openai"), "OPENAI_MODEL");
}

#[test]
fn test_model_env_var_unknown_fallback() {
    let registry = builtin_registry();
    assert_eq!(registry.model_env_var("nonexistent"), "LLM_MODEL");
}

#[test]
fn test_is_known() {
    let registry = builtin_registry();
    assert!(registry.is_known("nearai"));
    assert!(registry.is_known("openai"));
    assert!(registry.is_known("groq"));
    assert!(!registry.is_known("nonexistent"));
}

#[test]
fn test_all_providers_have_required_fields() {
    let providers = builtin_providers();
    for def in &providers {
        assert!(!def.id.is_empty(), "provider must have an id");
        assert!(!def.model_env.is_empty(), "{}: model_env required", def.id);
        assert!(
            !def.default_model.is_empty(),
            "{}: default_model required",
            def.id
        );
        assert!(
            !def.description.is_empty(),
            "{}: description required",
            def.id
        );
    }
}

/// Whether an OpenAI-completions provider must declare a
/// `default_base_url` (some hosts resolve their endpoint by other means).
fn requires_default_base_url(def: &ProviderDefinition) -> bool {
    let exempt = matches!(
        def.id.as_str(),
        "openai" | "openai_compatible" | "bedrock" | "cloudflare"
    );
    def.protocol == ProviderProtocol::OpenAiCompletions && !exempt
}

#[test]
fn test_openai_compatible_providers_have_base_url() {
    let providers = builtin_providers();
    for def in &providers {
        if requires_default_base_url(def) {
            assert!(
                def.default_base_url.is_some(),
                "{}: OpenAI-completions provider should have a default_base_url",
                def.id
            );
        }
    }
}

#[test]
fn test_models_filter_accessor() {
    let registry = builtin_registry();
    // Groq has models_filter: "chat"
    let groq = registry.find("groq").expect("groq should exist");
    let filter = groq
        .setup
        .as_ref()
        .and_then(|s| s.models_filter())
        .expect("groq should have models_filter");
    assert_eq!(filter, "chat");

    // OpenAI has no models_filter
    let openai = registry.find("openai").expect("openai should exist");
    assert!(
        openai
            .setup
            .as_ref()
            .and_then(|s| s.models_filter())
            .is_none(),
        "openai should not have models_filter"
    );

    // Ollama setup hint variant should return None
    let ollama = registry.find("ollama").expect("ollama should exist");
    assert!(
        ollama
            .setup
            .as_ref()
            .and_then(|s| s.models_filter())
            .is_none(),
        "ollama should not have models_filter"
    );
}

#[test]
fn test_unsupported_params_deserialized() {
    let providers = builtin_providers();

    // Tinfoil should have temperature in unsupported_params
    let tinfoil = providers.iter().find(|p| p.id == "tinfoil").unwrap();
    assert!(
        tinfoil
            .unsupported_params
            .contains(&"temperature".to_string()),
        "tinfoil should have 'temperature' in unsupported_params"
    );

    // OpenAI should also have temperature in unsupported_params
    let openai = providers.iter().find(|p| p.id == "openai").unwrap();
    assert!(
        openai
            .unsupported_params
            .contains(&"temperature".to_string()),
        "openai should have 'temperature' in unsupported_params"
    );

    // Providers without the field in JSON should deserialize to empty vec
    let groq = providers.iter().find(|p| p.id == "groq").unwrap();
    assert!(
        groq.unsupported_params.is_empty(),
        "groq should have empty unsupported_params (field absent in JSON)"
    );

    // All entries should only contain valid param names
    // (Invalid names should be rejected at deserialization time)
    for def in &providers {
        for param in &def.unsupported_params {
            assert!(
                !param.is_empty(),
                "{}: unsupported_params contains empty string",
                def.id
            );
            assert!(
                matches!(
                    param.as_str(),
                    "temperature" | "max_tokens" | "stop_sequences"
                ),
                "{}: unsupported_params contains invalid parameter '{}'",
                def.id,
                param
            );
        }
    }
}

#[test]
fn test_unsupported_params_validation_rejects_invalid() {
    // Invalid parameter names should cause deserialization error
    let invalid_json = r#"[{
        "id": "test",
        "protocol": "open_ai_completions",
        "model_env": "TEST_MODEL",
        "default_model": "test-model",
        "description": "Test provider",
        "unsupported_params": ["temperrature"]
    }]"#;

    let result: Result<Vec<ProviderDefinition>, _> = serde_json::from_str(invalid_json);
    assert!(
        result.is_err(),
        "should reject invalid parameter name 'temperrature'"
    );
    assert!(
        result.err().unwrap().to_string().contains("temperrature"),
        "error message should mention the invalid parameter"
    );
}

#[test]
fn test_all_builtin_api_key_providers_have_api_key_env() {
    // Every built-in provider with SetupHint::ApiKey must have api_key_env
    // set, otherwise inject_llm_keys_from_secrets can't map the secret.
    let providers = builtin_providers();
    for def in &providers {
        if let Some(SetupHint::ApiKey { .. }) = &def.setup {
            assert!(
                def.api_key_env.is_some(),
                "{}: ApiKey setup hint requires api_key_env to be set",
                def.id
            );
        }
    }
}
