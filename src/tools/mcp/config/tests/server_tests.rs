//! Tests for server/transport config types: validation, auth detection,
//! localhost URL checks, and serde round-trips.

use std::collections::HashMap;

use crate::tools::mcp::config::server::is_localhost_url;
use crate::tools::mcp::config::{
    EffectiveTransport, McpServerConfig, McpTransportConfig, OAuthConfig,
};

#[test]
fn test_is_localhost_url() {
    assert!(is_localhost_url("http://localhost:3000/path"));
    assert!(is_localhost_url("https://localhost/path"));
    assert!(is_localhost_url("http://127.0.0.1:8080"));
    assert!(is_localhost_url("http://127.0.0.1"));
    assert!(!is_localhost_url("https://notlocalhost.com/path"));
    assert!(!is_localhost_url("https://example-localhost.io"));
    assert!(!is_localhost_url("https://mcp.notion.com"));
    assert!(is_localhost_url("http://user:pass@localhost:3000/path"));
    // IPv6 loopback
    assert!(is_localhost_url("http://[::1]:8080/path"));
    assert!(is_localhost_url("http://[::1]/path"));
    assert!(!is_localhost_url("http://[::2]:8080/path"));
}

#[test]
fn test_server_config_validation() {
    // Valid HTTPS server
    let config = McpServerConfig::new("notion", "https://mcp.notion.com");
    assert!(config.validate().is_ok());

    // Valid localhost (allowed for dev)
    let config = McpServerConfig::new("local", "http://localhost:8080");
    assert!(config.validate().is_ok());

    // Invalid: empty name
    let config = McpServerConfig::new("", "https://example.com");
    assert!(config.validate().is_err());

    // Invalid: HTTP for remote server
    let config = McpServerConfig::new("remote", "http://mcp.example.com");
    assert!(config.validate().is_err());
}

#[test]
fn test_oauth_config_builder() {
    let oauth = OAuthConfig::new("client-123")
        .with_endpoints(
            "https://auth.example.com/authorize",
            "https://auth.example.com/token",
        )
        .with_scopes(vec!["read".to_string(), "write".to_string()]);

    assert_eq!(oauth.client_id, "client-123");
    assert!(oauth.authorization_url.is_some());
    assert!(oauth.token_url.is_some());
    assert_eq!(oauth.scopes.len(), 2);
    assert!(oauth.use_pkce);
}

#[test]
fn test_token_secret_names() {
    let config = McpServerConfig::new("notion", "https://mcp.notion.com");
    assert_eq!(config.token_secret_name(), "mcp_notion_access_token");
    assert_eq!(
        config.refresh_token_secret_name(),
        "mcp_notion_refresh_token"
    );
}

#[test]
fn test_requires_auth_with_oauth() {
    let config = McpServerConfig::new("notion", "https://mcp.notion.com")
        .with_oauth(OAuthConfig::new("client-123"));
    assert!(config.requires_auth());
}

#[test]
fn test_requires_auth_remote_https_without_oauth() {
    // Remote HTTPS servers need auth even without pre-configured OAuth (DCR)
    let config = McpServerConfig::new("github-copilot", "https://api.githubcopilot.com/mcp/");
    assert!(config.requires_auth());

    let config = McpServerConfig::new("notion", "https://mcp.notion.com");
    assert!(config.requires_auth());
}

#[test]
fn test_requires_auth_localhost_no_auth() {
    // Localhost servers are dev servers, no auth needed
    let config = McpServerConfig::new("local", "http://localhost:8080");
    assert!(!config.requires_auth());

    let config = McpServerConfig::new("local", "http://127.0.0.1:3000/mcp");
    assert!(!config.requires_auth());

    // Even HTTPS localhost doesn't require auth
    let config = McpServerConfig::new("local", "https://localhost:8443");
    assert!(!config.requires_auth());
}

#[test]
fn test_requires_auth_http_remote_no_auth() {
    // HTTP remote servers won't pass validation, but if they existed
    // they wouldn't trigger HTTPS auth detection
    let config = McpServerConfig::new("bad", "http://mcp.example.com");
    assert!(!config.requires_auth());
}

#[test]
fn test_stdio_config_creation() {
    let env = HashMap::from([("PATH".to_string(), "/usr/bin".to_string())]);
    let config = McpServerConfig::new_stdio(
        "my-server",
        "npx",
        vec!["-y".to_string(), "@modelcontextprotocol/server".to_string()],
        env.clone(),
    );

    assert_eq!(config.name, "my-server");
    assert!(config.url.is_empty());
    assert!(config.enabled);
    assert!(config.oauth.is_none());
    assert!(config.headers.is_empty());

    match &config.transport {
        Some(McpTransportConfig::Stdio {
            command,
            args,
            env: e,
        }) => {
            assert_eq!(command, "npx");
            assert_eq!(
                args,
                &["-y".to_string(), "@modelcontextprotocol/server".to_string()]
            );
            assert_eq!(e, &env);
        }
        other => panic!("Expected Stdio transport, got {:?}", other),
    }
}

#[test]
fn test_unix_config_creation() {
    let config = McpServerConfig::new_unix("local-server", "/tmp/mcp.sock");

    assert_eq!(config.name, "local-server");
    assert!(config.url.is_empty());
    assert!(config.enabled);

    match &config.transport {
        Some(McpTransportConfig::Unix { socket_path }) => {
            assert_eq!(socket_path, "/tmp/mcp.sock");
        }
        other => panic!("Expected Unix transport, got {:?}", other),
    }
}

#[test]
fn test_stdio_validation() {
    // Valid stdio config
    let config = McpServerConfig::new_stdio("server", "npx", vec![], HashMap::new());
    assert!(config.validate().is_ok());

    // Invalid: empty command
    let config = McpServerConfig::new_stdio("server", "", vec![], HashMap::new());
    assert!(config.validate().is_err());
    let err = config.validate().unwrap_err().to_string();
    assert!(
        err.contains("command"),
        "Error should mention command: {}",
        err
    );

    // Invalid: empty name
    let config = McpServerConfig::new_stdio("", "npx", vec![], HashMap::new());
    assert!(config.validate().is_err());
}

#[test]
fn test_unix_validation() {
    // Valid unix config
    let config = McpServerConfig::new_unix("server", "/tmp/mcp.sock");
    assert!(config.validate().is_ok());

    // Invalid: empty socket path
    let config = McpServerConfig::new_unix("server", "");
    assert!(config.validate().is_err());
    let err = config.validate().unwrap_err().to_string();
    assert!(
        err.contains("socket"),
        "Error should mention socket: {}",
        err
    );

    // Invalid: empty name
    let config = McpServerConfig::new_unix("", "/tmp/mcp.sock");
    assert!(config.validate().is_err());
}

#[test]
fn test_requires_auth_stdio_never() {
    // Stdio transport should never require auth, even with OAuth configured
    let mut config = McpServerConfig::new_stdio("server", "npx", vec![], HashMap::new());
    assert!(!config.requires_auth());

    // Even if OAuth is set, stdio doesn't use HTTP auth
    config.oauth = Some(OAuthConfig::new("client-123"));
    assert!(!config.requires_auth());
}

#[test]
fn test_requires_auth_unix_never() {
    // Unix transport should never require auth
    let mut config = McpServerConfig::new_unix("server", "/tmp/mcp.sock");
    assert!(!config.requires_auth());

    config.oauth = Some(OAuthConfig::new("client-123"));
    assert!(!config.requires_auth());
}

#[test]
fn test_custom_headers() {
    let headers = HashMap::from([
        ("X-Api-Key".to_string(), "secret".to_string()),
        ("Authorization".to_string(), "Bearer token".to_string()),
    ]);
    let config =
        McpServerConfig::new("server", "https://mcp.example.com").with_headers(headers.clone());

    assert_eq!(config.headers, headers);
    assert_eq!(config.headers.get("X-Api-Key").unwrap(), "secret");
}

#[test]
fn test_transport_config_serde_http() {
    let transport = McpTransportConfig::Http;
    let json = serde_json::to_string(&transport).unwrap();
    assert!(json.contains("\"transport\":\"http\""));

    let parsed: McpTransportConfig = serde_json::from_str(&json).unwrap();
    assert!(matches!(parsed, McpTransportConfig::Http));
}

#[test]
fn test_transport_config_serde_stdio() {
    let transport = McpTransportConfig::Stdio {
        command: "npx".to_string(),
        args: vec!["-y".to_string(), "server".to_string()],
        env: HashMap::from([("KEY".to_string(), "val".to_string())]),
    };
    let json = serde_json::to_string(&transport).unwrap();
    assert!(json.contains("\"transport\":\"stdio\""));
    assert!(json.contains("\"command\":\"npx\""));

    let parsed: McpTransportConfig = serde_json::from_str(&json).unwrap();
    match parsed {
        McpTransportConfig::Stdio { command, args, env } => {
            assert_eq!(command, "npx");
            assert_eq!(args, vec!["-y".to_string(), "server".to_string()]);
            assert_eq!(env.get("KEY").unwrap(), "val");
        }
        other => panic!("Expected Stdio, got {:?}", other),
    }
}

#[test]
fn test_transport_config_serde_unix() {
    let transport = McpTransportConfig::Unix {
        socket_path: "/tmp/mcp.sock".to_string(),
    };
    let json = serde_json::to_string(&transport).unwrap();
    assert!(json.contains("\"transport\":\"unix\""));
    assert!(json.contains("\"socket_path\":\"/tmp/mcp.sock\""));

    let parsed: McpTransportConfig = serde_json::from_str(&json).unwrap();
    match parsed {
        McpTransportConfig::Unix { socket_path } => {
            assert_eq!(socket_path, "/tmp/mcp.sock");
        }
        other => panic!("Expected Unix, got {:?}", other),
    }
}

#[test]
fn test_backward_compat_no_transport_field() {
    // Existing configs without transport field should still deserialize
    let json = r#"{
        "name": "notion",
        "url": "https://mcp.notion.com",
        "enabled": true
    }"#;
    let config: McpServerConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.name, "notion");
    assert_eq!(config.url, "https://mcp.notion.com");
    assert!(config.transport.is_none());
    assert!(config.headers.is_empty());
    assert!(matches!(
        config.effective_transport(),
        EffectiveTransport::Http
    ));
}

#[test]
fn test_config_roundtrip_with_transport() {
    // Test full roundtrip with stdio transport
    let config = McpServerConfig::new_stdio(
        "test-server",
        "node",
        vec!["server.js".to_string()],
        HashMap::from([("NODE_ENV".to_string(), "production".to_string())]),
    )
    .with_description("A test server");

    let json = serde_json::to_string_pretty(&config).unwrap();
    let parsed: McpServerConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.name, "test-server");
    assert!(parsed.url.is_empty());
    assert_eq!(parsed.description.as_deref(), Some("A test server"));

    match &parsed.transport {
        Some(McpTransportConfig::Stdio { command, args, env }) => {
            assert_eq!(command, "node");
            assert_eq!(args, &["server.js".to_string()]);
            assert_eq!(env.get("NODE_ENV").unwrap(), "production");
        }
        other => panic!("Expected Stdio transport, got {:?}", other),
    }

    // Test full roundtrip with unix transport
    let config = McpServerConfig::new_unix("unix-server", "/var/run/mcp.sock");
    let json = serde_json::to_string_pretty(&config).unwrap();
    let parsed: McpServerConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.name, "unix-server");
    match &parsed.transport {
        Some(McpTransportConfig::Unix { socket_path }) => {
            assert_eq!(socket_path, "/var/run/mcp.sock");
        }
        other => panic!("Expected Unix transport, got {:?}", other),
    }

    // Test roundtrip with HTTP + headers
    let headers = HashMap::from([("X-Custom".to_string(), "value".to_string())]);
    let config =
        McpServerConfig::new("http-server", "https://mcp.example.com").with_headers(headers);
    let json = serde_json::to_string_pretty(&config).unwrap();
    let parsed: McpServerConfig = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.name, "http-server");
    assert!(parsed.transport.is_none());
    assert_eq!(parsed.headers.get("X-Custom").unwrap(), "value");
}
