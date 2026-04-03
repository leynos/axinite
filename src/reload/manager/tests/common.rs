use std::net::SocketAddr;

use tempfile::TempDir;

use crate::config::HttpConfig;

/// Test case for address resolution scenarios.
pub(super) struct AddrTestCase {
    /// Host string to use in [`HttpConfig`].
    pub(super) host: &'static str,
    /// Port to use in [`HttpConfig`].
    pub(super) port: u16,
    /// Expected socket address for verification.
    pub(super) expected_addr: SocketAddr,
    /// Description for test output.
    pub(super) description: &'static str,
}

/// Helper to create a minimal test config with the given HTTP config.
pub(super) async fn test_config_with_http(
    http: Option<HttpConfig>,
) -> (TempDir, crate::config::Config) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let temp_db = temp_dir.path().join("test_reload.db");
    let skills_dir = temp_dir.path().join("skills");
    let installed_skills_dir = temp_dir.path().join("installed_skills");
    let mut config = crate::config::Config::for_testing(temp_db, skills_dir, installed_skills_dir)
        .await
        .expect("test config should build");
    config.channels.http = http;
    (temp_dir, config)
}

/// Construct an [`HttpConfig`] with the supplied host, port, and webhook secret.
pub(super) fn http_config(host: &str, port: u16, webhook_secret: Option<&str>) -> HttpConfig {
    HttpConfig {
        host: host.to_string(),
        port,
        user_id: "test_user".to_string(),
        webhook_secret: webhook_secret.map(|value| secrecy::SecretString::from(value.to_string())),
    }
}
