//! Behavioural tests for setup operation planning.

use proptest::prelude::*;

use super::error::SetupError;
use super::model::{
    CredentialName, EndpointField, EndpointKind, EndpointName, FieldKey, FieldKeyKind, SecretField,
    SetupField, SetupOperation,
};
use super::policy::{PlannedEffect, plan_setup};

fn secret(name: &str) -> CredentialName {
    CredentialName::parse(name).expect("valid secret name")
}

fn endpoint(name: &str) -> EndpointName {
    EndpointName::parse(name).expect("valid endpoint name")
}

fn kind(text: &str) -> EndpointKind {
    EndpointKind::parse(text).expect("valid endpoint kind")
}

fn secret_field(name: &str, optional: bool) -> SetupField {
    SetupField::Secret(SecretField {
        name: secret(name),
        prompt: "Prompt".to_string(),
        optional,
        auto_generate: false,
    })
}

fn endpoint_field(name: &str, optional: bool) -> SetupField {
    SetupField::Endpoint(EndpointField {
        name: endpoint(name),
        kind: kind("jmap"),
        prompt: "Endpoint".to_string(),
        optional,
    })
}

fn key(kind: FieldKeyKind, name: &str) -> FieldKey {
    match kind {
        FieldKeyKind::Secret => FieldKey::Secret(secret(name)),
        FieldKeyKind::Endpoint | FieldKeyKind::EndpointKind => FieldKey::Endpoint(endpoint(name)),
    }
}

// --- C3: mutation and failure semantics ----------------------------------

fn declarations() -> Vec<SetupField> {
    vec![
        secret_field("api_token", false),
        secret_field("optional_token", true),
        endpoint_field("calendar", false),
        endpoint_field("contacts", true),
    ]
}

fn endpoint_set(name: &str, url: &str) -> SetupOperation {
    SetupOperation::EndpointSet {
        name: endpoint(name),
        url: url.to_string(),
    }
}

fn secret_set(name: &str, value: &str) -> SetupOperation {
    SetupOperation::Secret {
        name: secret(name),
        value: value.to_string(),
    }
}

#[test]
fn omitted_fields_produce_no_effects() {
    let plan = plan_setup(&declarations(), Vec::new()).expect("empty plan");

    assert!(plan.effects().is_empty());
    assert_eq!(plan.required_endpoints().count(), 1);
}

#[test]
fn blank_secret_keeps_the_stored_value() {
    let plan = plan_setup(&declarations(), vec![secret_set("api_token", "   ")]).expect("valid");

    assert!(matches!(
        plan.effects(),
        [PlannedEffect::KeepSecret { name }] if name.as_str() == "api_token"
    ));
    assert!(!plan.effects()[0].mutates());
}

#[test]
fn endpoint_set_validates_then_replaces() {
    let plan = plan_setup(
        &declarations(),
        vec![endpoint_set("calendar", "https://mail.example.com/jmap/")],
    )
    .expect("valid");

    let [PlannedEffect::ReplaceEndpoint { name, kind, url }] = plan.effects() else {
        panic!("expected one endpoint replacement");
    };
    assert_eq!(name.as_str(), "calendar");
    assert_eq!(kind.as_str(), "jmap");
    assert_eq!(url.expose(), "https://mail.example.com/jmap/");
}

#[test]
fn endpoint_clear_is_an_idempotent_effect() {
    let clear = SetupOperation::EndpointClear {
        name: endpoint("calendar"),
    };
    let plan = plan_setup(&declarations(), vec![clear]).expect("valid");

    assert!(matches!(
        plan.effects(),
        [PlannedEffect::ClearEndpoint { name }] if name.as_str() == "calendar"
    ));
}

#[test]
fn empty_endpoint_set_is_rejected() {
    let result = plan_setup(&declarations(), vec![endpoint_set("calendar", "")]);

    assert!(matches!(result, Err(SetupError::EmptyEndpoint { .. })));
}

#[test]
fn invalid_endpoint_set_produces_no_plan() {
    let result = plan_setup(
        &declarations(),
        vec![
            secret_set("api_token", "value"),
            endpoint_set("calendar", "http://mail.example.com"),
        ],
    );

    assert!(matches!(result, Err(SetupError::InvalidEndpoint { .. })));
}

#[test]
fn undeclared_keys_are_rejected() {
    let result = plan_setup(
        &declarations(),
        vec![endpoint_set(
            "https://evil.example.com",
            "https://evil.example.com",
        )],
    );

    assert!(matches!(result, Err(SetupError::UndeclaredField { .. })));
}

#[test]
fn wrong_kind_operations_are_rejected() {
    let as_secret = SetupOperation::Secret {
        name: secret("calendar"),
        value: "https://example.com".to_string(),
    };
    assert!(matches!(
        plan_setup(&declarations(), vec![as_secret]),
        Err(SetupError::WrongFieldKind { .. })
    ));

    let as_endpoint = SetupOperation::EndpointClear {
        name: endpoint("api_token"),
    };
    assert!(matches!(
        plan_setup(&declarations(), vec![as_endpoint]),
        Err(SetupError::WrongFieldKind { .. })
    ));
}

#[test]
fn duplicate_operations_are_rejected() {
    let result = plan_setup(
        &declarations(),
        vec![
            endpoint_set("calendar", "https://one.example.com"),
            endpoint_set("calendar", "https://two.example.com"),
        ],
    );

    assert!(matches!(result, Err(SetupError::DuplicateOperation { .. })));
}

#[test]
fn secret_and_endpoint_with_the_same_text_do_not_alias() {
    let fields = vec![
        secret_field("shared", false),
        endpoint_field("shared", false),
    ];
    let plan = plan_setup(
        &fields,
        vec![
            secret_set("shared", "token"),
            endpoint_set("shared", "https://example.com"),
        ],
    )
    .expect("distinct stores");

    assert_eq!(plan.effects().len(), 2);
    assert_eq!(
        plan.effects()[0].field_key(),
        key(FieldKeyKind::Secret, "shared")
    );
    assert_eq!(
        plan.effects()[1].field_key(),
        key(FieldKeyKind::Endpoint, "shared")
    );
}

#[test]
fn operation_debug_redacts_submitted_values() {
    let operation = endpoint_set("calendar", "https://mail.example.com/secret");
    let rendered = format!("{operation:?}");

    assert!(!rendered.contains("example.com"));
    assert!(rendered.contains("[REDACTED]"));
}

// --- property tests ------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn accepted_operations_are_partitioned_exactly_once(
        secret_count in 0usize..4,
        endpoint_count in 0usize..3,
        clear_mask in 0u8..8,
    ) {
        let mut fields = Vec::new();
        let mut operations = Vec::new();
        let mut expected = Vec::new();

        for index in 0..secret_count {
            let name = format!("secret_{index}");
            fields.push(secret_field(&name, false));
            operations.push(secret_set(&name, "value"));
            expected.push(key(FieldKeyKind::Secret, &name));
        }
        for index in 0..endpoint_count {
            let name = format!("endpoint_{index}");
            fields.push(endpoint_field(&name, false));
            if clear_mask & (1 << index) != 0 {
                operations.push(SetupOperation::EndpointClear { name: endpoint(&name) });
            } else {
                operations.push(endpoint_set(&name, &format!("https://host{index}.example.com")));
            }
            expected.push(key(FieldKeyKind::Endpoint, &name));
        }

        let plan = plan_setup(&fields, operations).expect("generated plan is valid");
        let mut actual: Vec<FieldKey> = plan.effects().iter().map(PlannedEffect::field_key).collect();
        actual.sort_by_key(|field| (field.kind().to_string(), field.as_str().to_string()));
        expected.sort_by_key(|field| (field.kind().to_string(), field.as_str().to_string()));

        prop_assert_eq!(actual, expected);
    }
}
