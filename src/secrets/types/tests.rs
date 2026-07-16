//! Unit tests for secret reference and parameter types.

use crate::secrets::types::{CreateSecretParams, DecryptedSecret, SecretRef};

#[test]
fn test_secret_ref_creation() {
    let r = SecretRef::new("my_api_key").with_provider("openai");
    assert_eq!(r.name, "my_api_key");
    assert_eq!(r.provider, Some("openai".to_string()));
}

#[test]
fn test_decrypted_secret_redaction() {
    let secret = DecryptedSecret::from_bytes(b"super_secret_value".to_vec()).unwrap();
    let debug_str = format!("{:?}", secret);
    assert!(!debug_str.contains("super_secret_value"));
    assert!(debug_str.contains("REDACTED"));
}

#[test]
fn test_decrypted_secret_expose() {
    let secret = DecryptedSecret::from_bytes(b"test_value".to_vec()).unwrap();
    assert_eq!(secret.expose(), "test_value");
    assert_eq!(secret.len(), 10);
}

#[test]
fn test_create_params() {
    let params = CreateSecretParams::new("key", "value").with_provider("stripe");
    assert_eq!(params.name, "key");
    assert_eq!(params.provider, Some("stripe".to_string()));
}

#[test]
fn test_create_params_name_lowercased() {
    let params = CreateSecretParams::new("SLACK_BOT_TOKEN", "val");
    assert_eq!(params.name, "slack_bot_token");
}

#[test]
fn test_create_params_with_expiry() {
    use chrono::Utc;
    let expiry = Utc::now();
    let params = CreateSecretParams::new("key", "val").with_expiry(expiry);
    assert_eq!(params.expires_at, Some(expiry));
}

#[test]
fn test_secret_ref_without_provider() {
    let r = SecretRef::new("token");
    assert_eq!(r.name, "token");
    assert!(r.provider.is_none());
}

#[test]
fn test_secret_ref_serde_roundtrip() {
    let original = SecretRef::new("api_key").with_provider("openai");
    let json = serde_json::to_string(&original).unwrap();
    let deserialized: SecretRef = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.name, original.name);
    assert_eq!(deserialized.provider, original.provider);
}

#[test]
fn test_secret_ref_serde_without_provider() {
    let original = SecretRef::new("bare_token");
    let json = serde_json::to_string(&original).unwrap();
    assert!(json.contains("\"provider\":null"));
    let deserialized: SecretRef = serde_json::from_str(&json).unwrap();
    assert!(deserialized.provider.is_none());
}

#[test]
fn test_credential_location_serde_roundtrip_bearer() {
    use crate::secrets::types::CredentialLocation;
    let loc = CredentialLocation::AuthorizationBearer;
    let json = serde_json::to_string(&loc).unwrap();
    let back: CredentialLocation = serde_json::from_str(&json).unwrap();
    assert!(matches!(back, CredentialLocation::AuthorizationBearer));
}

#[test]
fn test_credential_location_serde_roundtrip_basic() {
    use crate::secrets::types::CredentialLocation;
    let loc = CredentialLocation::AuthorizationBasic {
        username: "admin".to_string(),
    };
    let json = serde_json::to_string(&loc).unwrap();
    let back: CredentialLocation = serde_json::from_str(&json).unwrap();
    match back {
        CredentialLocation::AuthorizationBasic { username } => {
            assert_eq!(username, "admin");
        }
        _ => panic!("expected AuthorizationBasic"),
    }
}

#[test]
fn test_credential_location_serde_roundtrip_header() {
    use crate::secrets::types::CredentialLocation;
    let loc = CredentialLocation::Header {
        name: "X-Api-Key".to_string(),
        prefix: Some("Token".to_string()),
    };
    let json = serde_json::to_string(&loc).unwrap();
    let back: CredentialLocation = serde_json::from_str(&json).unwrap();
    match back {
        CredentialLocation::Header { name, prefix } => {
            assert_eq!(name, "X-Api-Key");
            assert_eq!(prefix, Some("Token".to_string()));
        }
        _ => panic!("expected Header"),
    }
}

#[test]
fn test_credential_location_serde_roundtrip_query_param() {
    use crate::secrets::types::CredentialLocation;
    let loc = CredentialLocation::QueryParam {
        name: "access_token".to_string(),
    };
    let json = serde_json::to_string(&loc).unwrap();
    let back: CredentialLocation = serde_json::from_str(&json).unwrap();
    match back {
        CredentialLocation::QueryParam { name } => assert_eq!(name, "access_token"),
        _ => panic!("expected QueryParam"),
    }
}

#[test]
fn test_credential_location_serde_roundtrip_url_path() {
    use crate::secrets::types::CredentialLocation;
    let loc = CredentialLocation::UrlPath {
        placeholder: "{api_key}".to_string(),
    };
    let json = serde_json::to_string(&loc).unwrap();
    let back: CredentialLocation = serde_json::from_str(&json).unwrap();
    match back {
        CredentialLocation::UrlPath { placeholder } => assert_eq!(placeholder, "{api_key}"),
        _ => panic!("expected UrlPath"),
    }
}

#[test]
fn test_credential_location_default_is_bearer() {
    use crate::secrets::types::CredentialLocation;
    let loc = CredentialLocation::default();
    assert!(matches!(loc, CredentialLocation::AuthorizationBearer));
}

#[test]
fn test_credential_mapping_bearer_constructor() {
    use crate::secrets::types::CredentialMapping;
    let m = CredentialMapping::bearer("my_token", "*.example.com");
    assert_eq!(m.secret_name, "my_token");
    assert!(matches!(
        m.location,
        crate::secrets::types::CredentialLocation::AuthorizationBearer
    ));
    assert_eq!(m.host_patterns, vec!["*.example.com".to_string()]);
}

#[test]
fn test_credential_mapping_header_constructor() {
    use crate::secrets::types::CredentialMapping;
    let m = CredentialMapping::header("key", "X-Custom", "api.host.com");
    assert_eq!(m.secret_name, "key");
    match &m.location {
        crate::secrets::types::CredentialLocation::Header { name, prefix } => {
            assert_eq!(name, "X-Custom");
            assert!(prefix.is_none());
        }
        _ => panic!("expected Header"),
    }
    assert_eq!(m.host_patterns, vec!["api.host.com".to_string()]);
}

#[test]
fn test_credential_mapping_serde_roundtrip() {
    use crate::secrets::types::CredentialMapping;
    let original = CredentialMapping::bearer("tok", "*.api.com");
    let json = serde_json::to_string(&original).unwrap();
    let back: CredentialMapping = serde_json::from_str(&json).unwrap();
    assert_eq!(back.secret_name, "tok");
    assert_eq!(back.host_patterns, vec!["*.api.com".to_string()]);
}

#[test]
fn test_decrypted_secret_invalid_utf8() {
    let result = DecryptedSecret::from_bytes(vec![0xFF, 0xFE, 0x00]);
    assert!(result.is_err());
}

#[test]
fn test_decrypted_secret_empty() {
    let secret = DecryptedSecret::from_bytes(Vec::new()).unwrap();
    assert!(secret.is_empty());
    assert_eq!(secret.len(), 0);
    assert_eq!(secret.expose(), "");
}

#[test]
fn test_decrypted_secret_clone() {
    let original = DecryptedSecret::from_bytes(b"cloneable".to_vec()).unwrap();
    let cloned = original.clone();
    assert_eq!(cloned.expose(), "cloneable");
    assert_eq!(cloned.len(), original.len());
}

#[test]
fn test_secret_debug_redacts_fields() {
    use chrono::Utc;
    use uuid::Uuid;
    let secret = crate::secrets::types::Secret {
        id: Uuid::nil(),
        user_id: "user1".to_string(),
        name: "test_key".to_string(),
        encrypted_value: vec![1, 2, 3],
        key_salt: vec![4, 5, 6],
        provider: Some("aws".to_string()),
        expires_at: None,
        last_used_at: None,
        usage_count: 5,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };
    let debug = format!("{:?}", secret);
    assert!(debug.contains("REDACTED"));
    assert!(!debug.contains("[1, 2, 3]"));
    assert!(!debug.contains("[4, 5, 6]"));
    assert!(debug.contains("test_key"));
}

#[test]
fn test_secret_error_display() {
    use crate::secrets::types::SecretError;
    assert_eq!(
        SecretError::NotFound("foo".into()).to_string(),
        "Secret not found: foo"
    );
    assert_eq!(SecretError::Expired.to_string(), "Secret has expired");
    assert_eq!(
        SecretError::InvalidMasterKey.to_string(),
        "Invalid master key"
    );
    assert_eq!(
        SecretError::InvalidUtf8.to_string(),
        "Secret value is not valid UTF-8"
    );
    assert_eq!(
        SecretError::AccessDenied.to_string(),
        "Secret access denied for tool"
    );
}
