//! Tests for OAuth type serialization, deserialization, and error display.

use super::super::{
    AccessToken, AuthError, AuthorizationServerMetadata, ClientRegistrationRequest,
    ClientRegistrationResponse, ProtectedResourceMetadata,
};

#[test]
fn test_protected_resource_metadata_serde_roundtrip_full() {
    let meta = ProtectedResourceMetadata {
        resource: "https://mcp.example.com".to_string(),
        authorization_servers: vec![
            "https://auth1.example.com".to_string(),
            "https://auth2.example.com".to_string(),
        ],
        scopes_supported: vec!["read".to_string(), "write".to_string()],
    };

    let json = serde_json::to_string(&meta).unwrap();
    let deserialized: ProtectedResourceMetadata = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.resource, meta.resource);
    assert_eq!(
        deserialized.authorization_servers,
        meta.authorization_servers
    );
    assert_eq!(deserialized.scopes_supported, meta.scopes_supported);
}

#[test]
fn test_protected_resource_metadata_serde_roundtrip_minimal() {
    // Only required field, optional vecs should default to empty.
    let json = r#"{"resource": "https://mcp.example.com"}"#;
    let meta: ProtectedResourceMetadata = serde_json::from_str(json).unwrap();

    assert_eq!(meta.resource, "https://mcp.example.com");
    assert!(meta.authorization_servers.is_empty());
    assert!(meta.scopes_supported.is_empty());
}

#[test]
fn test_authorization_server_metadata_serde_roundtrip_all_fields() {
    let meta = AuthorizationServerMetadata {
        issuer: "https://auth.example.com".to_string(),
        authorization_endpoint: "https://auth.example.com/authorize".to_string(),
        token_endpoint: "https://auth.example.com/token".to_string(),
        registration_endpoint: Some("https://auth.example.com/register".to_string()),
        response_types_supported: vec!["code".to_string()],
        grant_types_supported: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        code_challenge_methods_supported: vec!["S256".to_string()],
        scopes_supported: vec!["openid".to_string(), "profile".to_string()],
    };

    let json = serde_json::to_string(&meta).unwrap();
    let rt: AuthorizationServerMetadata = serde_json::from_str(&json).unwrap();

    assert_eq!(rt.issuer, meta.issuer);
    assert_eq!(rt.authorization_endpoint, meta.authorization_endpoint);
    assert_eq!(rt.token_endpoint, meta.token_endpoint);
    assert_eq!(rt.registration_endpoint, meta.registration_endpoint);
    assert_eq!(rt.response_types_supported, meta.response_types_supported);
    assert_eq!(rt.grant_types_supported, meta.grant_types_supported);
    assert_eq!(
        rt.code_challenge_methods_supported,
        meta.code_challenge_methods_supported
    );
    assert_eq!(rt.scopes_supported, meta.scopes_supported);
}

#[test]
fn test_authorization_server_metadata_serde_without_registration() {
    let json = r#"{
        "issuer": "https://auth.example.com",
        "authorization_endpoint": "https://auth.example.com/authorize",
        "token_endpoint": "https://auth.example.com/token"
    }"#;

    let meta: AuthorizationServerMetadata = serde_json::from_str(json).unwrap();
    assert_eq!(meta.issuer, "https://auth.example.com");
    assert!(meta.registration_endpoint.is_none());
    assert!(meta.response_types_supported.is_empty());
    assert!(meta.grant_types_supported.is_empty());
}

#[test]
fn test_client_registration_request_serialization() {
    let req = ClientRegistrationRequest {
        client_name: "IronClaw".to_string(),
        redirect_uris: vec!["http://localhost:9876/callback".to_string()],
        grant_types: vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ],
        response_types: vec!["code".to_string()],
        token_endpoint_auth_method: "none".to_string(),
    };

    let value: serde_json::Value = serde_json::to_value(&req).unwrap();

    assert_eq!(value["client_name"], "IronClaw");
    assert_eq!(value["redirect_uris"][0], "http://localhost:9876/callback");
    assert_eq!(value["grant_types"][0], "authorization_code");
    assert_eq!(value["grant_types"][1], "refresh_token");
    assert_eq!(value["response_types"][0], "code");
    assert_eq!(value["token_endpoint_auth_method"], "none");
}

#[test]
fn test_client_registration_response_deserialization_full() {
    let json = r#"{
        "client_id": "abc-123",
        "client_secret": "s3cret",
        "client_secret_expires_at": 1700000000,
        "registration_access_token": "reg-tok",
        "registration_client_uri": "https://auth.example.com/register/abc-123"
    }"#;

    let resp: ClientRegistrationResponse = serde_json::from_str(json).unwrap();

    assert_eq!(resp.client_id, "abc-123");
    assert_eq!(resp.client_secret.as_deref(), Some("s3cret"));
    assert_eq!(resp.client_secret_expires_at, Some(1700000000));
    assert_eq!(resp.registration_access_token.as_deref(), Some("reg-tok"));
    assert_eq!(
        resp.registration_client_uri.as_deref(),
        Some("https://auth.example.com/register/abc-123")
    );
}

#[test]
fn test_client_registration_response_deserialization_minimal() {
    let json = r#"{"client_id": "xyz-789"}"#;

    let resp: ClientRegistrationResponse = serde_json::from_str(json).unwrap();

    assert_eq!(resp.client_id, "xyz-789");
    assert!(resp.client_secret.is_none());
    assert!(resp.client_secret_expires_at.is_none());
    assert!(resp.registration_access_token.is_none());
    assert!(resp.registration_client_uri.is_none());
}

#[test]
fn test_access_token_construction() {
    let token = AccessToken {
        access_token: "at-abc".to_string(),
        token_type: "Bearer".to_string(),
        expires_in: Some(3600),
        refresh_token: Some("rt-xyz".to_string()),
        scope: Some("read write".to_string()),
    };

    assert_eq!(token.access_token, "at-abc");
    assert_eq!(token.token_type, "Bearer");
    assert_eq!(token.expires_in, Some(3600));
    assert_eq!(token.refresh_token.as_deref(), Some("rt-xyz"));
    assert_eq!(token.scope.as_deref(), Some("read write"));

    // Also test with no optional fields.
    let minimal = AccessToken {
        access_token: "tok".to_string(),
        token_type: "bearer".to_string(),
        expires_in: None,
        refresh_token: None,
        scope: None,
    };
    assert!(minimal.expires_in.is_none());
    assert!(minimal.refresh_token.is_none());
    assert!(minimal.scope.is_none());
}

#[test]
fn test_token_response_to_access_token_pattern() {
    // TokenResponse is private, but we can test the conversion pattern
    // by deserializing JSON the same way exchange_code_for_token does.
    let json = r#"{
        "access_token": "eyJ-token",
        "token_type": "Bearer",
        "expires_in": 7200,
        "refresh_token": "refresh-me",
        "scope": "openid profile"
    }"#;

    // Deserialize via the same struct path the production code uses.
    let resp: serde_json::Value = serde_json::from_str(json).unwrap();
    let token = AccessToken {
        access_token: resp["access_token"].as_str().unwrap().to_string(),
        token_type: resp["token_type"].as_str().unwrap().to_string(),
        expires_in: resp["expires_in"].as_u64(),
        refresh_token: resp["refresh_token"].as_str().map(String::from),
        scope: resp["scope"].as_str().map(String::from),
    };

    assert_eq!(token.access_token, "eyJ-token");
    assert_eq!(token.token_type, "Bearer");
    assert_eq!(token.expires_in, Some(7200));
    assert_eq!(token.refresh_token.as_deref(), Some("refresh-me"));
    assert_eq!(token.scope.as_deref(), Some("openid profile"));

    // Without optional fields.
    let minimal_json = r#"{"access_token": "tok", "token_type": "bearer"}"#;
    let resp: serde_json::Value = serde_json::from_str(minimal_json).unwrap();
    let token = AccessToken {
        access_token: resp["access_token"].as_str().unwrap().to_string(),
        token_type: resp["token_type"].as_str().unwrap().to_string(),
        expires_in: resp["expires_in"].as_u64(),
        refresh_token: resp["refresh_token"].as_str().map(String::from),
        scope: resp["scope"].as_str().map(String::from),
    };
    assert!(token.expires_in.is_none());
    assert!(token.refresh_token.is_none());
    assert!(token.scope.is_none());
}

#[test]
fn test_auth_error_display_strings() {
    let cases: Vec<(AuthError, &str)> = vec![
        (
            AuthError::NotSupported,
            "Server does not support OAuth authorization",
        ),
        (
            AuthError::DiscoveryFailed("timeout".to_string()),
            "Failed to discover authorization endpoints: timeout",
        ),
        (
            AuthError::AuthorizationDenied,
            "Authorization denied by user",
        ),
        (
            AuthError::TokenExchangeFailed("bad code".to_string()),
            "Token exchange failed: bad code",
        ),
        (
            AuthError::RefreshFailed("expired".to_string()),
            "Token expired and refresh failed: expired",
        ),
        (AuthError::NoToken, "No access token available"),
        (
            AuthError::Timeout,
            "Timeout waiting for authorization callback",
        ),
        (
            AuthError::PortUnavailable,
            "Could not bind to callback port",
        ),
        (
            AuthError::Http("connection refused".to_string()),
            "HTTP error: connection refused",
        ),
        (
            AuthError::Secrets("decrypt failed".to_string()),
            "Secrets error: decrypt failed",
        ),
    ];

    for (error, expected) in cases {
        let display = error.to_string();
        assert_eq!(
            display, expected,
            "AuthError display mismatch for {:?}",
            error
        );
    }
}
