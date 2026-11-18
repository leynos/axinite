//! Behavioural tests for setup declaration normalization.

use super::error::SetupError;
use super::manifest::{normalize_channel_setup, normalize_tool_setup};
use super::model::{EndpointField, SecretField, SetupField};

use crate::channels::wasm::{
    AutoGenerateSchema, SecretSetupSchema, SetupSchema as ChannelSetupSchema,
};
use crate::tools::wasm::{ToolSecretSetupSchema, ToolSetupFieldSchema};

fn secret_declaration(fields: &[SetupField], index: usize) -> &SecretField {
    match &fields[index] {
        SetupField::Secret(field) => field,
        SetupField::Endpoint(_) => panic!("expected a secret declaration"),
    }
}

fn endpoint_declaration(fields: &[SetupField], index: usize) -> &EndpointField {
    match &fields[index] {
        SetupField::Endpoint(field) => field,
        SetupField::Secret(_) => panic!("expected an endpoint declaration"),
    }
}

// --- C1: typed declaration and routing -----------------------------------

#[test]
fn typed_tool_manifest_routes_secret_and_endpoint() {
    let typed = vec![
        ToolSetupFieldSchema::Secret {
            name: "api_token".to_string(),
            prompt: "API token".to_string(),
            optional: false,
        },
        ToolSetupFieldSchema::Endpoint {
            name: "calendar".to_string(),
            kind: "jmap".to_string(),
            prompt: "Calendar endpoint".to_string(),
            optional: true,
        },
    ];

    let declarations = normalize_tool_setup(&[], &typed).expect("typed manifest normalizes");

    assert_eq!(declarations.len(), 2);
    assert_eq!(
        secret_declaration(&declarations, 0).name.as_str(),
        "api_token"
    );
    let calendar = endpoint_declaration(&declarations, 1);
    assert_eq!(calendar.name.as_str(), "calendar");
    assert_eq!(calendar.kind.as_str(), "jmap");
    assert!(calendar.optional);
}

#[test]
fn legacy_tool_manifest_normalizes_to_secret_fields() {
    let legacy = vec![ToolSecretSetupSchema {
        name: "api_token".to_string(),
        prompt: "API token".to_string(),
        optional: true,
    }];

    let declarations = normalize_tool_setup(&legacy, &[]).expect("legacy manifest normalizes");

    assert_eq!(declarations.len(), 1);
    let token = secret_declaration(&declarations, 0);
    assert_eq!(token.name.as_str(), "api_token");
    assert!(token.optional);
    assert!(!token.auto_generate);
}

#[test]
fn channel_manifest_normalizes_auto_generation() {
    let setup = ChannelSetupSchema {
        required_secrets: vec![
            SecretSetupSchema {
                name: "telegram_bot_token".to_string(),
                prompt: "Bot token".to_string(),
                validation: None,
                optional: false,
                auto_generate: None,
            },
            SecretSetupSchema {
                name: "webhook_secret".to_string(),
                prompt: "Webhook secret".to_string(),
                validation: None,
                optional: true,
                auto_generate: Some(AutoGenerateSchema { length: 32 }),
            },
        ],
        validation_endpoint: None,
        setup_url: None,
    };

    let declarations = normalize_channel_setup(&setup).expect("channel manifest normalizes");

    assert_eq!(declarations.len(), 2);
    assert!(matches!(&declarations[0], SetupField::Secret(field) if !field.auto_generate));
    assert!(matches!(&declarations[1], SetupField::Secret(field) if field.auto_generate));
}

#[test]
fn mixing_legacy_and_typed_declarations_is_rejected() {
    let legacy = vec![ToolSecretSetupSchema {
        name: "api_token".to_string(),
        prompt: "API token".to_string(),
        optional: false,
    }];
    let typed = vec![ToolSetupFieldSchema::Secret {
        name: "other".to_string(),
        prompt: "Other".to_string(),
        optional: false,
    }];

    assert_eq!(
        normalize_tool_setup(&legacy, &typed),
        Err(SetupError::MixedLegacyAndTypedDeclarations)
    );
}

#[test]
fn duplicate_endpoint_names_are_rejected() {
    let typed = vec![
        ToolSetupFieldSchema::Endpoint {
            name: "calendar".to_string(),
            kind: "jmap".to_string(),
            prompt: "Calendar".to_string(),
            optional: false,
        },
        ToolSetupFieldSchema::Endpoint {
            name: "calendar".to_string(),
            kind: "caldav".to_string(),
            prompt: "Calendar again".to_string(),
            optional: false,
        },
    ];

    assert!(matches!(
        normalize_tool_setup(&[], &typed),
        Err(SetupError::DuplicateField { .. })
    ));
}

#[test]
fn secret_aliases_differing_only_by_case_are_rejected() {
    let typed = vec![
        ToolSetupFieldSchema::Secret {
            name: "API_TOKEN".to_string(),
            prompt: "Upper".to_string(),
            optional: false,
        },
        ToolSetupFieldSchema::Secret {
            name: "api_token".to_string(),
            prompt: "Lower".to_string(),
            optional: false,
        },
    ];

    assert!(matches!(
        normalize_tool_setup(&[], &typed),
        Err(SetupError::DuplicateField { .. })
    ));
}

#[test]
fn unknown_typed_tag_is_rejected() {
    let json = r#"{"fields": [{"type": "url", "name": "calendar", "prompt": "Calendar"}]}"#;

    assert!(serde_json::from_str::<crate::tools::wasm::ToolSetupSchema>(json).is_err());
}

#[test]
fn unknown_typed_member_is_rejected() {
    let json = r#"{"fields": [{"type": "endpoint", "name": "calendar", "kind": "jmap",
        "prompt": "Calendar", "url": "https://example.com"}]}"#;

    assert!(serde_json::from_str::<crate::tools::wasm::ToolSetupSchema>(json).is_err());
}
