//! Configuration, settings-mapping, and credential extraction tests.

use axinite::import::openclaw::reader::OpenClawReader;
use axinite::import::openclaw::settings;

use super::harness::setup_full_openclaw_test_env;

#[test]
fn test_full_config_extraction() {
    let (_temp, openclaw_path) = setup_full_openclaw_test_env().expect("setup failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");
    let config = reader.read_config().expect("config read failed");

    // Verify LLM config
    assert_eq!(
        config.llm.as_ref().map(|c| c.provider.clone()),
        Some(Some("openai".to_string()))
    );
    assert_eq!(
        config.llm.as_ref().map(|c| c.model.clone()),
        Some(Some("gpt-4-turbo".to_string()))
    );

    // Verify embeddings config
    assert_eq!(
        config.embeddings.as_ref().map(|c| c.model.clone()),
        Some(Some("text-embedding-3-large".to_string()))
    );

    // Verify custom settings preserved
    assert!(config.other_settings.contains_key("custom_setting"));
}

#[test]
fn test_settings_mapping_to_axinite_format() {
    let (_temp, openclaw_path) = setup_full_openclaw_test_env().expect("setup failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");
    let config = reader.read_config().expect("config read failed");

    let settings_map = settings::map_openclaw_config_to_settings(&config);

    // Verify key mappings
    assert!(settings_map.contains_key("llm.backend"));
    assert!(settings_map.contains_key("llm.selected_model"));
    assert!(settings_map.contains_key("embeddings.model"));
    assert!(settings_map.contains_key("custom_setting"));

    // Verify values
    assert_eq!(
        settings_map.get("llm.backend").and_then(|v| v.as_str()),
        Some("openai")
    );
}

#[test]
fn test_credentials_extraction() {
    let (_temp, openclaw_path) = setup_full_openclaw_test_env().expect("setup failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");
    let config = reader.read_config().expect("config read failed");

    let creds = settings::extract_credentials(&config);

    // Should extract 2 credentials (llm_api_key + embeddings_api_key)
    assert_eq!(creds.len(), 2);

    // Verify names (order may vary, so check both are present)
    let names: Vec<_> = creds.iter().map(|(name, _)| name).collect();
    assert!(names.contains(&&"llm_api_key".to_string()));
    assert!(names.contains(&&"embeddings_api_key".to_string()));

    // Verify credentials are wrapped in SecretString (not exposed in debug)
    for (_name, secret) in creds {
        let debug_str = format!("{:?}", secret);
        assert!(!debug_str.contains("sk-test-key"));
        assert!(!debug_str.contains("sk-embed-key"));
    }
}

#[test]
fn test_credentials_never_logged() {
    let (_temp, openclaw_path) = setup_full_openclaw_test_env().expect("setup failed");

    let reader = OpenClawReader::new(&openclaw_path).expect("reader creation failed");
    let config = reader.read_config().expect("config read failed");

    let creds = settings::extract_credentials(&config);

    // Verify actual secrets are not exposed
    for (_name, secret) in creds {
        let secret_debug = format!("{:?}", secret);
        // Should NOT contain the actual API keys
        assert!(!secret_debug.contains("sk-test-key-12345"));
        assert!(!secret_debug.contains("sk-embed-key-67890"));
    }
}
