//! Tests for wizard construction and small display helpers.

use super::super::*;

#[test]
fn test_wizard_creation() {
    let wizard = SetupWizard::new();
    assert!(!wizard.config.skip_auth);
    assert!(!wizard.config.channels_only);
}

#[test]
fn test_wizard_with_config() {
    let config = SetupConfig {
        skip_auth: true,
        channels_only: false,
        provider_only: false,
        quick: false,
    };
    let wizard = SetupWizard::with_config(config);
    assert!(wizard.config.skip_auth);
}

#[test]
#[cfg(feature = "postgres")]
fn test_mask_password_in_url() {
    assert_eq!(
        mask_password_in_url("postgres://user:secret@localhost/db"),
        "postgres://user:****@localhost/db"
    );

    // URL without password
    assert_eq!(
        mask_password_in_url("postgres://localhost/db"),
        "postgres://localhost/db"
    );
}

#[test]
fn test_capitalize_first() {
    assert_eq!(capitalize_first("telegram"), "Telegram");
    assert_eq!(capitalize_first("CAPS"), "CAPS");
    assert_eq!(capitalize_first(""), "");
}

#[test]
fn test_mask_api_key() {
    assert_eq!(
        mask_api_key("sk-ant-api03-abcdef1234567890"),
        "sk-ant...7890"
    );
    assert_eq!(mask_api_key("short"), "shor...");
    assert_eq!(mask_api_key("exactly12ch"), "exac...");
    assert_eq!(mask_api_key("exactly12chr"), "exactl...2chr");
    assert_eq!(mask_api_key(""), "...");
    // Multi-byte chars should not panic
    assert_eq!(mask_api_key("日本語キー"), "日本語キ...");
}
