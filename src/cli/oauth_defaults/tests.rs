use std::collections::HashMap;
use std::sync::MutexGuard;

use rstest::{fixture, rstest};

use super::{
    build_oauth_url, build_platform_state, builtin_credentials, callback_host, callback_url,
    is_loopback_host, landing_html, strip_instance_prefix, use_gateway_callback,
};
use crate::config::helpers::ENV_MUTEX;

struct EnvVarGuard {
    _lock: MutexGuard<'static, ()>,
    key: &'static str,
    original: Option<String>,
}

impl EnvVarGuard {
    fn new(key: &'static str) -> Self {
        let lock = ENV_MUTEX.lock().expect("env mutex poisoned");
        let original = std::env::var(key).ok();
        Self {
            _lock: lock,
            key,
            original,
        }
    }

    fn remove(&self) {
        unsafe {
            std::env::remove_var(self.key);
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        unsafe {
            if let Some(value) = &self.original {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }
}

#[fixture]
fn oauth_callback_host_guard() -> EnvVarGuard {
    EnvVarGuard::new("OAUTH_CALLBACK_HOST")
}

#[test]
fn test_is_loopback_host() {
    assert!(is_loopback_host("127.0.0.1"));
    assert!(is_loopback_host("127.0.0.2"));
    assert!(is_loopback_host("127.255.255.254"));
    assert!(is_loopback_host("::1"));
    assert!(is_loopback_host("localhost"));
    assert!(is_loopback_host("LOCALHOST"));
    assert!(!is_loopback_host("203.0.113.10"));
    assert!(!is_loopback_host("my-server.example.com"));
    assert!(!is_loopback_host("0.0.0.0"));
}

#[rstest]
fn test_callback_host_default(oauth_callback_host_guard: EnvVarGuard) {
    oauth_callback_host_guard.remove();
    assert_eq!(callback_host(), "127.0.0.1");
}

#[test]
fn test_callback_host_env_override() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    let original_host = std::env::var("OAUTH_CALLBACK_HOST").ok();
    let original_url = std::env::var("IRONCLAW_OAUTH_CALLBACK_URL").ok();
    unsafe {
        std::env::set_var("OAUTH_CALLBACK_HOST", "203.0.113.10");
        std::env::remove_var("IRONCLAW_OAUTH_CALLBACK_URL");
    }
    assert_eq!(callback_host(), "203.0.113.10");
    let url = callback_url();
    assert!(url.contains("203.0.113.10"), "url was: {url}");
    unsafe {
        if let Some(val) = original_host {
            std::env::set_var("OAUTH_CALLBACK_HOST", val);
        } else {
            std::env::remove_var("OAUTH_CALLBACK_HOST");
        }
        if let Some(val) = original_url {
            std::env::set_var("IRONCLAW_OAUTH_CALLBACK_URL", val);
        }
    }
}

#[test]
fn test_callback_url_default() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    let original_url = std::env::var("IRONCLAW_OAUTH_CALLBACK_URL").ok();
    let original_host = std::env::var("OAUTH_CALLBACK_HOST").ok();
    unsafe {
        std::env::remove_var("IRONCLAW_OAUTH_CALLBACK_URL");
        std::env::remove_var("OAUTH_CALLBACK_HOST");
    }
    assert_eq!(callback_url(), "http://127.0.0.1:9876");
    unsafe {
        if let Some(val) = original_url {
            std::env::set_var("IRONCLAW_OAUTH_CALLBACK_URL", val);
        }
        if let Some(val) = original_host {
            std::env::set_var("OAUTH_CALLBACK_HOST", val);
        }
    }
}

#[test]
fn test_callback_url_env_override() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    let original = std::env::var("IRONCLAW_OAUTH_CALLBACK_URL").ok();
    unsafe {
        std::env::set_var(
            "IRONCLAW_OAUTH_CALLBACK_URL",
            "https://myserver.example.com:9876",
        );
    }
    assert_eq!(callback_url(), "https://myserver.example.com:9876");
    unsafe {
        if let Some(val) = original {
            std::env::set_var("IRONCLAW_OAUTH_CALLBACK_URL", val);
        } else {
            std::env::remove_var("IRONCLAW_OAUTH_CALLBACK_URL");
        }
    }
}

#[test]
fn test_unknown_provider_returns_none() {
    assert!(builtin_credentials("unknown_token").is_none());
}

#[test]
fn test_google_returns_based_on_compile_env() {
    let creds = builtin_credentials("google_oauth_token")
        .expect("expected built-in Google OAuth credentials to be present");
    assert!(!creds.client_id.is_empty());
    assert!(!creds.client_secret.is_empty());
}

#[test]
fn test_landing_html_success_contains_key_elements() {
    let html = landing_html("Google", true);
    assert!(html.contains("Google Connected"));
    assert!(html.contains("charset"));
    assert!(html.contains("IronClaw"));
    assert!(html.contains("#22c55e"));
    assert!(!html.contains("Failed"));
}

#[test]
fn test_landing_html_escapes_provider_name() {
    let html = landing_html("<script>alert(1)</script>", true);
    assert!(!html.contains("<script>"));
    assert!(html.contains("&lt;script&gt;"));
}

#[test]
fn test_landing_html_error_contains_key_elements() {
    let html = landing_html("Notion", false);
    assert!(html.contains("Authorization Failed"));
    assert!(html.contains("charset"));
    assert!(html.contains("IronClaw"));
    assert!(html.contains("#ef4444"));
    assert!(!html.contains("Connected"));
}

#[test]
fn test_build_oauth_url_basic() {
    let result = build_oauth_url(
        "https://accounts.google.com/o/oauth2/auth",
        "my-client-id",
        "http://localhost:9876/callback",
        &["openid".to_string(), "email".to_string()],
        false,
        &HashMap::new(),
    );

    assert!(
        result
            .url
            .starts_with("https://accounts.google.com/o/oauth2/auth?")
    );
    assert!(result.url.contains("client_id=my-client-id"));
    assert!(result.url.contains("response_type=code"));
    assert!(result.url.contains("redirect_uri="));
    assert!(result.url.contains("scope=openid%20email"));
    assert!(result.url.contains("state="));
    assert!(result.code_verifier.is_none());
    assert!(!result.state.is_empty());
}

#[test]
fn test_build_oauth_url_with_pkce() {
    let result = build_oauth_url(
        "https://auth.example.com/authorize",
        "client-123",
        "http://localhost:9876/callback",
        &[],
        true,
        &HashMap::new(),
    );

    assert!(result.url.contains("code_challenge="));
    assert!(result.url.contains("code_challenge_method=S256"));
    assert!(result.code_verifier.is_some());
    assert!(!result.code_verifier.expect("pkce verifier").is_empty());
}

#[test]
fn test_build_oauth_url_with_extra_params() {
    let mut extra = HashMap::new();
    extra.insert("access_type".to_string(), "offline".to_string());
    extra.insert("prompt".to_string(), "consent".to_string());

    let result = build_oauth_url(
        "https://auth.example.com/authorize",
        "client-123",
        "http://localhost:9876/callback",
        &["read".to_string()],
        false,
        &extra,
    );

    assert!(result.url.contains("access_type=offline"));
    assert!(result.url.contains("prompt=consent"));
}

#[test]
fn test_build_oauth_url_state_is_unique() {
    let result1 = build_oauth_url(
        "https://auth.example.com/authorize",
        "client",
        "http://localhost:9876/callback",
        &[],
        false,
        &HashMap::new(),
    );
    let result2 = build_oauth_url(
        "https://auth.example.com/authorize",
        "client",
        "http://localhost:9876/callback",
        &[],
        false,
        &HashMap::new(),
    );
    assert_ne!(result1.state, result2.state);
}

#[test]
fn test_use_gateway_callback_false_by_default() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    let original = std::env::var("IRONCLAW_OAUTH_CALLBACK_URL").ok();
    unsafe {
        std::env::remove_var("IRONCLAW_OAUTH_CALLBACK_URL");
    }
    assert!(!use_gateway_callback());
    unsafe {
        if let Some(val) = original {
            std::env::set_var("IRONCLAW_OAUTH_CALLBACK_URL", val);
        }
    }
}

#[test]
fn test_use_gateway_callback_true_for_hosted() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    let original = std::env::var("IRONCLAW_OAUTH_CALLBACK_URL").ok();
    unsafe {
        std::env::set_var(
            "IRONCLAW_OAUTH_CALLBACK_URL",
            "https://kind-deer.agent1.near.ai",
        );
    }
    assert!(use_gateway_callback());
    unsafe {
        if let Some(val) = original {
            std::env::set_var("IRONCLAW_OAUTH_CALLBACK_URL", val);
        } else {
            std::env::remove_var("IRONCLAW_OAUTH_CALLBACK_URL");
        }
    }
}

#[test]
fn test_use_gateway_callback_false_for_localhost() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    let original = std::env::var("IRONCLAW_OAUTH_CALLBACK_URL").ok();
    unsafe {
        std::env::set_var("IRONCLAW_OAUTH_CALLBACK_URL", "http://127.0.0.1:3001");
    }
    assert!(!use_gateway_callback());
    unsafe {
        if let Some(val) = original {
            std::env::set_var("IRONCLAW_OAUTH_CALLBACK_URL", val);
        } else {
            std::env::remove_var("IRONCLAW_OAUTH_CALLBACK_URL");
        }
    }
}

#[test]
fn test_use_gateway_callback_false_for_empty() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    let original = std::env::var("IRONCLAW_OAUTH_CALLBACK_URL").ok();
    unsafe {
        std::env::set_var("IRONCLAW_OAUTH_CALLBACK_URL", "");
    }
    assert!(!use_gateway_callback());
    unsafe {
        if let Some(val) = original {
            std::env::set_var("IRONCLAW_OAUTH_CALLBACK_URL", val);
        } else {
            std::env::remove_var("IRONCLAW_OAUTH_CALLBACK_URL");
        }
    }
}

#[test]
fn test_build_platform_state_with_instance() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    let original = std::env::var("IRONCLAW_INSTANCE_NAME").ok();
    unsafe {
        std::env::set_var("IRONCLAW_INSTANCE_NAME", "kind-deer");
    }
    assert_eq!(build_platform_state("abc123"), "kind-deer:abc123");
    unsafe {
        if let Some(val) = original {
            std::env::set_var("IRONCLAW_INSTANCE_NAME", val);
        } else {
            std::env::remove_var("IRONCLAW_INSTANCE_NAME");
        }
    }
}

#[test]
fn test_build_platform_state_without_instance() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    let original = std::env::var("IRONCLAW_INSTANCE_NAME").ok();
    let original_oc = std::env::var("OPENCLAW_INSTANCE_NAME").ok();
    unsafe {
        std::env::remove_var("IRONCLAW_INSTANCE_NAME");
        std::env::remove_var("OPENCLAW_INSTANCE_NAME");
    }
    assert_eq!(build_platform_state("abc123"), "abc123");
    unsafe {
        if let Some(val) = original {
            std::env::set_var("IRONCLAW_INSTANCE_NAME", val);
        }
        if let Some(val) = original_oc {
            std::env::set_var("OPENCLAW_INSTANCE_NAME", val);
        }
    }
}

#[test]
fn test_build_platform_state_with_openclaw_instance() {
    let _guard = ENV_MUTEX.lock().expect("env mutex poisoned");
    let original_ic = std::env::var("IRONCLAW_INSTANCE_NAME").ok();
    let original_oc = std::env::var("OPENCLAW_INSTANCE_NAME").ok();
    unsafe {
        std::env::remove_var("IRONCLAW_INSTANCE_NAME");
        std::env::set_var("OPENCLAW_INSTANCE_NAME", "quiet-lion");
    }
    assert_eq!(build_platform_state("xyz789"), "quiet-lion:xyz789");
    unsafe {
        if let Some(val) = original_ic {
            std::env::set_var("IRONCLAW_INSTANCE_NAME", val);
        }
        if let Some(val) = original_oc {
            std::env::set_var("OPENCLAW_INSTANCE_NAME", val);
        } else {
            std::env::remove_var("OPENCLAW_INSTANCE_NAME");
        }
    }
}

#[test]
fn test_strip_instance_prefix_with_colon() {
    assert_eq!(strip_instance_prefix("kind-deer:abc123"), "abc123");
    assert_eq!(strip_instance_prefix("my-instance:xyz"), "xyz");
}

#[test]
fn test_strip_instance_prefix_without_colon() {
    assert_eq!(strip_instance_prefix("abc123"), "abc123");
    assert_eq!(strip_instance_prefix(""), "");
}
