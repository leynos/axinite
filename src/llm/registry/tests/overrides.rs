//! Tests for user provider overrides and the `selectable()` wizard listing.

use crate::llm::registry::{ProviderDefinition, ProviderProtocol, ProviderRegistry, SetupHint};

#[test]
fn test_selectable_has_setup_hints() {
    let registry = ProviderRegistry::new(
        serde_json::from_str(include_str!("../../../../providers.json")).unwrap(),
    );
    let selectable = registry.selectable();
    assert!(!selectable.is_empty());
    for def in &selectable {
        assert!(
            def.setup.is_some(),
            "selectable provider {} must have setup hint",
            def.id
        );
    }
}

#[test]
fn test_user_override_wins() {
    let builtins: Vec<ProviderDefinition> =
        serde_json::from_str(include_str!("../../../../providers.json")).unwrap();
    let mut all = builtins;
    // Simulate user overriding tinfoil with a different default model
    all.push(ProviderDefinition {
        id: "tinfoil".to_string(),
        aliases: vec![],
        protocol: ProviderProtocol::OpenAiCompletions,
        default_base_url: Some("https://custom.tinfoil.example/v1".to_string()),
        base_url_env: None,
        base_url_required: false,
        api_key_env: Some("TINFOIL_API_KEY".to_string()),
        api_key_required: true,
        model_env: "TINFOIL_MODEL".to_string(),
        default_model: "custom-model".to_string(),
        description: "Custom tinfoil".to_string(),
        extra_headers_env: None,
        setup: None,
        unsupported_params: vec![],
    });
    let registry = ProviderRegistry::new(all);
    let tf = registry.find("tinfoil").expect("tinfoil should exist");
    assert_eq!(tf.default_model, "custom-model", "user override should win");
}

#[test]
fn test_selectable_user_override_adds_setup() {
    // A built-in provider without setup hint should NOT appear in selectable().
    // But if a user override adds a setup hint, it SHOULD appear.
    let mut providers: Vec<ProviderDefinition> = vec![ProviderDefinition {
        id: "custom".to_string(),
        aliases: vec![],
        protocol: ProviderProtocol::OpenAiCompletions,
        default_base_url: Some("http://localhost/v1".to_string()),
        base_url_env: None,
        base_url_required: false,
        api_key_env: None,
        api_key_required: false,
        model_env: "CUSTOM_MODEL".to_string(),
        default_model: "m1".to_string(),
        description: "No setup".to_string(),
        extra_headers_env: None,
        setup: None, // no setup hint
        unsupported_params: vec![],
    }];

    let registry = ProviderRegistry::new(providers.clone());
    assert!(
        registry.selectable().is_empty(),
        "provider without setup should not be selectable"
    );

    // User override adds a setup hint
    providers.push(ProviderDefinition {
        id: "custom".to_string(),
        aliases: vec![],
        protocol: ProviderProtocol::OpenAiCompletions,
        default_base_url: Some("http://localhost/v1".to_string()),
        base_url_env: None,
        base_url_required: false,
        api_key_env: Some("CUSTOM_API_KEY".to_string()),
        api_key_required: true,
        model_env: "CUSTOM_MODEL".to_string(),
        default_model: "m1".to_string(),
        description: "Now with setup".to_string(),
        extra_headers_env: None,
        setup: Some(SetupHint::ApiKey {
            secret_name: "llm_custom_api_key".to_string(),
            key_url: None,
            display_name: "Custom".to_string(),
            can_list_models: false,
            models_filter: None,
        }),
        unsupported_params: vec![],
    });

    let registry = ProviderRegistry::new(providers);
    let selectable = registry.selectable();
    assert_eq!(
        selectable.len(),
        1,
        "user override with setup should appear"
    );
    assert_eq!(selectable[0].id, "custom");
    assert_eq!(
        selectable[0].description, "Now with setup",
        "should use the overridden definition"
    );
}

#[test]
fn test_selectable_user_override_removes_setup() {
    // If a built-in has setup but user override removes it, it should
    // NOT appear in selectable().
    let providers = vec![
        ProviderDefinition {
            id: "provider_a".to_string(),
            aliases: vec![],
            protocol: ProviderProtocol::OpenAiCompletions,
            default_base_url: Some("http://a/v1".to_string()),
            base_url_env: None,
            base_url_required: false,
            api_key_env: Some("A_KEY".to_string()),
            api_key_required: true,
            model_env: "A_MODEL".to_string(),
            default_model: "m1".to_string(),
            description: "Has setup".to_string(),
            extra_headers_env: None,
            setup: Some(SetupHint::ApiKey {
                secret_name: "a".to_string(),
                key_url: None,
                display_name: "A".to_string(),
                can_list_models: false,
                models_filter: None,
            }),
            unsupported_params: vec![],
        },
        // User override removes setup
        ProviderDefinition {
            id: "provider_a".to_string(),
            aliases: vec![],
            protocol: ProviderProtocol::OpenAiCompletions,
            default_base_url: Some("http://a/v1".to_string()),
            base_url_env: None,
            base_url_required: false,
            api_key_env: Some("A_KEY".to_string()),
            api_key_required: false,
            model_env: "A_MODEL".to_string(),
            default_model: "m1".to_string(),
            description: "No setup now".to_string(),
            extra_headers_env: None,
            setup: None,
            unsupported_params: vec![],
        },
    ];

    let registry = ProviderRegistry::new(providers);
    assert!(
        registry.selectable().is_empty(),
        "user override removing setup should exclude from selectable"
    );
    // But find() should still work (uses the override)
    let def = registry
        .find("provider_a")
        .expect("should still be findable");
    assert_eq!(def.description, "No setup now");
}

#[test]
fn test_selectable_preserves_order_with_dedup() {
    // If providers A, B, C are defined, and a user override for B comes
    // later, selectable() should return A, B, C (not A, C, B).
    let providers = vec![
        ProviderDefinition {
            id: "aaa".to_string(),
            aliases: vec![],
            protocol: ProviderProtocol::OpenAiCompletions,
            default_base_url: Some("http://a/v1".to_string()),
            base_url_env: None,
            base_url_required: false,
            api_key_env: None,
            api_key_required: false,
            model_env: "A".to_string(),
            default_model: "m".to_string(),
            description: "A".to_string(),
            extra_headers_env: None,
            setup: Some(SetupHint::Ollama {
                display_name: "A".to_string(),
                can_list_models: false,
            }),
            unsupported_params: vec![],
        },
        ProviderDefinition {
            id: "bbb".to_string(),
            aliases: vec![],
            protocol: ProviderProtocol::OpenAiCompletions,
            default_base_url: Some("http://b/v1".to_string()),
            base_url_env: None,
            base_url_required: false,
            api_key_env: None,
            api_key_required: false,
            model_env: "B".to_string(),
            default_model: "m".to_string(),
            description: "B-original".to_string(),
            extra_headers_env: None,
            setup: Some(SetupHint::Ollama {
                display_name: "B".to_string(),
                can_list_models: false,
            }),
            unsupported_params: vec![],
        },
        ProviderDefinition {
            id: "ccc".to_string(),
            aliases: vec![],
            protocol: ProviderProtocol::OpenAiCompletions,
            default_base_url: Some("http://c/v1".to_string()),
            base_url_env: None,
            base_url_required: false,
            api_key_env: None,
            api_key_required: false,
            model_env: "C".to_string(),
            default_model: "m".to_string(),
            description: "C".to_string(),
            extra_headers_env: None,
            setup: Some(SetupHint::Ollama {
                display_name: "C".to_string(),
                can_list_models: false,
            }),
            unsupported_params: vec![],
        },
        // User override for B
        ProviderDefinition {
            id: "bbb".to_string(),
            aliases: vec![],
            protocol: ProviderProtocol::OpenAiCompletions,
            default_base_url: Some("http://b-new/v1".to_string()),
            base_url_env: None,
            base_url_required: false,
            api_key_env: None,
            api_key_required: false,
            model_env: "B".to_string(),
            default_model: "m".to_string(),
            description: "B-override".to_string(),
            extra_headers_env: None,
            setup: Some(SetupHint::Ollama {
                display_name: "B".to_string(),
                can_list_models: false,
            }),
            unsupported_params: vec![],
        },
    ];

    let registry = ProviderRegistry::new(providers);
    let selectable = registry.selectable();
    let ids: Vec<&str> = selectable.iter().map(|d| d.id.as_str()).collect();
    assert_eq!(ids, vec!["aaa", "bbb", "ccc"], "order should be preserved");
    assert_eq!(
        selectable[1].description, "B-override",
        "should use the overridden definition"
    );
}
