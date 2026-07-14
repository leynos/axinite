//! Unit tests for WIT version compatibility and OAuth refresh config
//! resolution from capabilities files.

use crate::testing::credentials::{TEST_OAUTH_CLIENT_ID, TEST_OAUTH_CLIENT_SECRET};
use crate::tools::wasm::loader::check_wit_version_compat;
use crate::tools::wasm::loader::tool_loader::resolve_oauth_refresh_config;

#[test]
fn wit_version_compat_none_is_ok() {
    // Pre-versioning extensions (no wit_version declared) should always pass
    assert!(check_wit_version_compat("test", None, "0.2.0").is_ok());
}

#[test]
fn wit_version_compat_exact_match() {
    assert!(check_wit_version_compat("test", Some("0.2.0"), "0.2.0").is_ok());
}

#[test]
fn wit_version_compat_patch_older_ok() {
    // Extension on older patch of same minor is compatible
    assert!(check_wit_version_compat("test", Some("0.2.0"), "0.2.1").is_ok());
}

#[test]
fn wit_version_compat_minor_mismatch_0x() {
    // For 0.x, different minor is breaking
    assert!(check_wit_version_compat("test", Some("0.1.0"), "0.2.0").is_err());
    assert!(check_wit_version_compat("test", Some("0.3.0"), "0.2.0").is_err());
}

#[test]
fn wit_version_compat_major_mismatch() {
    assert!(check_wit_version_compat("test", Some("1.0.0"), "2.0.0").is_err());
}

#[test]
fn wit_version_compat_extension_newer_than_host() {
    assert!(check_wit_version_compat("test", Some("0.2.1"), "0.2.0").is_err());
}

#[test]
fn wit_version_compat_invalid_version() {
    assert!(check_wit_version_compat("test", Some("not-a-version"), "0.2.0").is_err());
}

#[test]
fn test_resolve_oauth_refresh_config_with_oauth() {
    use crate::tools::wasm::capabilities_schema::{
        AuthCapabilitySchema, CapabilitiesFile, OAuthConfigSchema,
    };

    let caps = CapabilitiesFile {
        auth: Some(AuthCapabilitySchema {
            secret_name: "google_oauth_token".to_string(),
            provider: Some("google".to_string()),
            oauth: Some(OAuthConfigSchema {
                authorization_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
                token_url: "https://oauth2.googleapis.com/token".to_string(),
                client_id: Some(TEST_OAUTH_CLIENT_ID.to_string()),
                client_secret: Some(TEST_OAUTH_CLIENT_SECRET.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let config = resolve_oauth_refresh_config(&caps);
    assert!(config.is_some());

    let config = config.unwrap();
    assert_eq!(config.token_url, "https://oauth2.googleapis.com/token");
    assert_eq!(config.client_id, TEST_OAUTH_CLIENT_ID);
    assert_eq!(
        config.client_secret,
        Some(TEST_OAUTH_CLIENT_SECRET.to_string())
    );
    assert_eq!(config.secret_name, "google_oauth_token");
    assert_eq!(config.provider, Some("google".to_string()));
}

#[test]
fn test_resolve_oauth_refresh_config_no_auth() {
    use crate::tools::wasm::capabilities_schema::CapabilitiesFile;

    let caps = CapabilitiesFile::default();
    let config = resolve_oauth_refresh_config(&caps);
    assert!(config.is_none());
}

#[test]
fn test_resolve_oauth_refresh_config_no_oauth() {
    use crate::tools::wasm::capabilities_schema::{AuthCapabilitySchema, CapabilitiesFile};

    let caps = CapabilitiesFile {
        auth: Some(AuthCapabilitySchema {
            secret_name: "manual_token".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    };

    let config = resolve_oauth_refresh_config(&caps);
    assert!(config.is_none());
}

#[test]
fn test_resolve_oauth_refresh_config_no_client_id() {
    use crate::tools::wasm::capabilities_schema::{
        AuthCapabilitySchema, CapabilitiesFile, OAuthConfigSchema,
    };

    // A non-Google provider with no client_id anywhere should return None
    let caps = CapabilitiesFile {
        auth: Some(AuthCapabilitySchema {
            secret_name: "unknown_provider_token".to_string(),
            oauth: Some(OAuthConfigSchema {
                authorization_url: "https://example.com/auth".to_string(),
                token_url: "https://example.com/token".to_string(),
                // No client_id, no client_id_env, no builtin
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let config = resolve_oauth_refresh_config(&caps);
    assert!(config.is_none());
}

#[test]
fn test_resolve_oauth_refresh_config_builtin_google() {
    use crate::tools::wasm::capabilities_schema::{
        AuthCapabilitySchema, CapabilitiesFile, OAuthConfigSchema,
    };

    // google_oauth_token should fall back to built-in credentials
    let caps = CapabilitiesFile {
        auth: Some(AuthCapabilitySchema {
            secret_name: "google_oauth_token".to_string(),
            provider: Some("google".to_string()),
            oauth: Some(OAuthConfigSchema {
                authorization_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
                token_url: "https://oauth2.googleapis.com/token".to_string(),
                // No inline client_id, should fall back to builtin
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };

    let config = resolve_oauth_refresh_config(&caps);
    assert!(config.is_some());
    let config = config.unwrap();
    assert!(!config.client_id.is_empty());
    assert!(config.client_secret.is_some());
}
