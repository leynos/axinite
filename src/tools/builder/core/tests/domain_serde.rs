//! Serde round-trip tests for builder domain types and default
//! configuration values.

use super::super::{
    BuildPhase, BuildRequirement, BuilderConfig, Language, ProjectName, SoftwareType,
};

#[test]
fn test_software_type_serde_roundtrip() {
    let variants = [
        SoftwareType::WasmTool,
        SoftwareType::CliBinary,
        SoftwareType::Library,
        SoftwareType::Script,
        SoftwareType::WebService,
    ];
    let expected_strings = [
        "\"wasm_tool\"",
        "\"cli_binary\"",
        "\"library\"",
        "\"script\"",
        "\"web_service\"",
    ];
    for (variant, expected) in variants.iter().zip(expected_strings.iter()) {
        let json = serde_json::to_string(variant).expect("serialize SoftwareType variant");
        assert_eq!(&json, expected, "serialization mismatch for {:?}", variant);
        let deserialized: SoftwareType =
            serde_json::from_str(&json).expect("deserialize SoftwareType");
        assert_eq!(
            &deserialized, variant,
            "roundtrip mismatch for {:?}",
            variant
        );
    }
}

#[test]
fn test_language_serde_roundtrip() {
    let variants = [
        Language::Rust,
        Language::Python,
        Language::TypeScript,
        Language::JavaScript,
        Language::Go,
        Language::Bash,
    ];
    let expected_strings = [
        "\"rust\"",
        "\"python\"",
        "\"type_script\"",
        "\"java_script\"",
        "\"go\"",
        "\"bash\"",
    ];
    for (variant, expected) in variants.iter().zip(expected_strings.iter()) {
        let json = serde_json::to_string(variant).expect("serialize Language variant");
        assert_eq!(&json, expected, "serialization mismatch for {:?}", variant);
        let deserialized: Language = serde_json::from_str(&json).expect("deserialize Language");
        assert_eq!(
            &deserialized, variant,
            "roundtrip mismatch for {:?}",
            variant
        );
    }
}

#[test]
fn test_build_requirement_serde_roundtrip() {
    let req = BuildRequirement {
        name: ProjectName::new("my_tool").expect("valid project name"),
        description: "A tool that does stuff".into(),
        software_type: SoftwareType::WasmTool,
        language: Language::Rust,
        input_spec: Some("JSON object with 'query' field".into()),
        output_spec: Some("JSON object with 'result' field".into()),
        dependencies: vec!["serde".into(), "reqwest".into()],
        capabilities: vec!["http".into(), "workspace".into()],
    };
    super::assertions::assert_build_requirement_roundtrip(&req);
}

#[test]
fn test_build_requirement_serde_optional_fields_none() {
    let req = BuildRequirement {
        name: ProjectName::new("minimal").expect("valid project name"),
        description: "Bare minimum".into(),
        software_type: SoftwareType::Script,
        language: Language::Bash,
        input_spec: None,
        output_spec: None,
        dependencies: vec![],
        capabilities: vec![],
    };
    let json = serde_json::to_string(&req).expect("serialize BuildRequirement");
    let deserialized: BuildRequirement =
        serde_json::from_str(&json).expect("deserialize BuildRequirement");
    super::assertions::assert_optional_fields_none(&deserialized);
}

#[test]
fn test_builder_config_default_sensible_values() {
    let config = BuilderConfig::default();
    super::assertions::assert_builder_config_defaults(&config);
}

#[test]
fn test_build_phase_serde_roundtrip() {
    let variants = [
        BuildPhase::Analyzing,
        BuildPhase::Scaffolding,
        BuildPhase::Implementing,
        BuildPhase::Building,
        BuildPhase::Testing,
        BuildPhase::Fixing,
        BuildPhase::Validating,
        BuildPhase::Registering,
        BuildPhase::Packaging,
        BuildPhase::Complete,
        BuildPhase::Failed,
    ];
    for variant in &variants {
        let json = serde_json::to_string(variant).expect("serialize BuildPhase variant");
        let deserialized: BuildPhase = serde_json::from_str(&json).expect("deserialize BuildPhase");
        assert_eq!(
            &deserialized, variant,
            "roundtrip mismatch for {:?}",
            variant
        );
    }
}
