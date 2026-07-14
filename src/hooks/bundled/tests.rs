//! Unit tests for bundled hook event handling.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use crate::hooks::{HookError, HookEvent, HookOutcome, HookPoint, HookRegistry};

use super::config::{
    HookBundleConfig, HookBundleError, HookRuleConfig, OutboundWebhookConfig,
    RegexReplacementConfig, timeout_from_ms,
};
use super::net_policy::dispatch_client_for_target;
use super::register_bundle;
use super::rule::RuleHook;
use super::webhook::OutboundWebhookHook;

fn inbound_event(content: &str) -> HookEvent {
    HookEvent::Inbound {
        user_id: "user-1".to_string(),
        channel: "test".to_string(),
        content: content.to_string(),
        thread_id: None,
    }
}

#[test]
fn test_parse_bundle_array_shorthand() {
    let value = serde_json::json!([
        {
            "name": "append-bang",
            "points": ["beforeInbound"],
            "append": "!"
        }
    ]);

    let parsed = HookBundleConfig::from_value(&value).unwrap();
    assert_eq!(parsed.rules.len(), 1);
    assert!(parsed.outbound_webhooks.is_empty());
}

#[tokio::test]
async fn test_rule_hook_modifies_content() {
    let registry = Arc::new(HookRegistry::new());

    let bundle = HookBundleConfig {
        rules: vec![HookRuleConfig {
            name: "redact-secret".to_string(),
            points: vec![HookPoint::BeforeInbound],
            priority: None,
            failure_mode: None,
            timeout_ms: None,
            when_regex: None,
            reject_reason: None,
            replacements: vec![RegexReplacementConfig {
                pattern: "secret".to_string(),
                replacement: "[redacted]".to_string(),
            }],
            prepend: None,
            append: None,
        }],
        outbound_webhooks: vec![],
    };

    let summary = register_bundle(&registry, "workspace:hooks/hooks.json", bundle).await;
    assert_eq!(summary.hooks, 1);
    assert_eq!(summary.errors, 0);

    let result = registry
        .run(&inbound_event("contains secret here"))
        .await
        .unwrap();
    match result {
        HookOutcome::Continue {
            modified: Some(value),
        } => {
            assert_eq!(value, "contains [redacted] here");
        }
        other => panic!("expected modified output, got {other:?}"),
    }
}

#[tokio::test]
async fn test_rule_hook_rejects() {
    let registry = Arc::new(HookRegistry::new());

    let bundle = HookBundleConfig {
        rules: vec![HookRuleConfig {
            name: "block-forbidden".to_string(),
            points: vec![HookPoint::BeforeInbound],
            priority: None,
            failure_mode: None,
            timeout_ms: None,
            when_regex: Some("forbidden".to_string()),
            reject_reason: Some("forbidden content".to_string()),
            replacements: vec![],
            prepend: None,
            append: None,
        }],
        outbound_webhooks: vec![],
    };

    let summary = register_bundle(&registry, "plugin:tool:test", bundle).await;
    assert_eq!(summary.hooks, 1);

    let result = registry.run(&inbound_event("this is forbidden")).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        HookError::Rejected { reason } if reason == "forbidden content"
    ));
}

#[tokio::test]
async fn test_outbound_webhook_hook_registers() {
    let registry = Arc::new(HookRegistry::new());

    let bundle = HookBundleConfig {
        rules: vec![],
        outbound_webhooks: vec![OutboundWebhookConfig {
            name: "notify".to_string(),
            points: vec![HookPoint::BeforeInbound],
            url: "https://example.com/hook".to_string(),
            headers: HashMap::new(),
            timeout_ms: Some(1000),
            priority: None,
            max_in_flight: None,
        }],
    };

    let summary = register_bundle(&registry, "workspace:hooks/webhook.hook.json", bundle).await;
    assert_eq!(summary.outbound_webhooks, 1);

    // Should return immediately regardless of webhook delivery result.
    let result = registry.run(&inbound_event("hello")).await;
    assert!(result.is_ok());
}

#[test]
fn test_timeout_from_ms_rejects_zero() {
    let err = timeout_from_ms(Some(0), "hook").unwrap_err();
    assert!(matches!(err, HookBundleError::InvalidTimeout { .. }));
}

#[test]
fn test_timeout_from_ms_rejects_above_limit() {
    let err = timeout_from_ms(Some(30_001), "hook").unwrap_err();
    assert!(matches!(err, HookBundleError::InvalidTimeout { .. }));
}

#[test]
fn test_rule_hook_requires_points() {
    let config = HookRuleConfig {
        name: "invalid".to_string(),
        points: vec![],
        priority: None,
        failure_mode: None,
        timeout_ms: None,
        when_regex: None,
        reject_reason: None,
        replacements: vec![],
        prepend: None,
        append: None,
    };

    let err = RuleHook::from_config("workspace:hooks/hooks.json", config).unwrap_err();
    assert!(matches!(err, HookBundleError::MissingHookPoints { .. }));
}

#[test]
fn test_invalid_webhook_scheme_rejected() {
    let config = OutboundWebhookConfig {
        name: "notify".to_string(),
        points: vec![HookPoint::BeforeInbound],
        url: "http://example.com/hook".to_string(),
        headers: HashMap::new(),
        timeout_ms: None,
        priority: None,
        max_in_flight: None,
    };

    let err = OutboundWebhookHook::from_config("workspace:hooks/hooks.json", config).unwrap_err();
    assert!(matches!(err, HookBundleError::InvalidWebhookScheme { .. }));
}

#[test]
fn test_private_webhook_host_rejected() {
    let config = OutboundWebhookConfig {
        name: "notify".to_string(),
        points: vec![HookPoint::BeforeInbound],
        url: "https://127.0.0.1/hook".to_string(),
        headers: HashMap::new(),
        timeout_ms: None,
        priority: None,
        max_in_flight: None,
    };

    let err = OutboundWebhookHook::from_config("workspace:hooks/hooks.json", config).unwrap_err();
    assert!(matches!(err, HookBundleError::ForbiddenWebhookHost { .. }));
}

#[test]
fn test_mapped_ipv4_webhook_host_rejected() {
    let config = OutboundWebhookConfig {
        name: "notify".to_string(),
        points: vec![HookPoint::BeforeInbound],
        url: "https://[::ffff:127.0.0.1]/hook".to_string(),
        headers: HashMap::new(),
        timeout_ms: None,
        priority: None,
        max_in_flight: None,
    };

    let err = OutboundWebhookHook::from_config("workspace:hooks/hooks.json", config).unwrap_err();
    assert!(matches!(err, HookBundleError::ForbiddenWebhookHost { .. }));
}

#[test]
fn test_restricted_webhook_header_rejected() {
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), "Bearer token".to_string());

    let config = OutboundWebhookConfig {
        name: "notify".to_string(),
        points: vec![HookPoint::BeforeInbound],
        url: "https://example.com/hook".to_string(),
        headers,
        timeout_ms: None,
        priority: None,
        max_in_flight: None,
    };

    let err = OutboundWebhookHook::from_config("workspace:hooks/hooks.json", config).unwrap_err();
    assert!(matches!(
        err,
        HookBundleError::ForbiddenWebhookHeader { .. }
    ));
}

#[test]
fn test_zero_max_in_flight_rejected() {
    let config = OutboundWebhookConfig {
        name: "notify".to_string(),
        points: vec![HookPoint::BeforeInbound],
        url: "https://example.com/hook".to_string(),
        headers: HashMap::new(),
        timeout_ms: None,
        priority: None,
        max_in_flight: Some(0),
    };

    let err = OutboundWebhookHook::from_config("workspace:hooks/hooks.json", config).unwrap_err();
    assert!(matches!(
        err,
        HookBundleError::InvalidWebhookMaxInFlight { .. }
    ));
}

#[tokio::test]
async fn test_runtime_target_validation_blocks_private_ip() {
    let base_client = reqwest::Client::builder().build().unwrap();
    let err = dispatch_client_for_target(
        &base_client,
        "https://127.0.0.1/hook",
        Duration::from_secs(1),
    )
    .await
    .unwrap_err();
    assert!(err.contains("blocked IP"));
}

#[tokio::test]
async fn test_runtime_target_validation_allows_public_ip() {
    let base_client = reqwest::Client::builder().build().unwrap();
    let result =
        dispatch_client_for_target(&base_client, "https://1.1.1.1/hook", Duration::from_secs(1))
            .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_rule_guard_no_match_is_passthrough() {
    let registry = Arc::new(HookRegistry::new());

    let bundle = HookBundleConfig {
        rules: vec![HookRuleConfig {
            name: "guarded-rewrite".to_string(),
            points: vec![HookPoint::BeforeInbound],
            priority: None,
            failure_mode: None,
            timeout_ms: None,
            when_regex: Some("forbidden".to_string()),
            reject_reason: None,
            replacements: vec![RegexReplacementConfig {
                pattern: "hello".to_string(),
                replacement: "hi".to_string(),
            }],
            prepend: None,
            append: None,
        }],
        outbound_webhooks: vec![],
    };

    register_bundle(&registry, "workspace:hooks/hooks.json", bundle).await;
    let result = registry.run(&inbound_event("hello world")).await.unwrap();
    assert!(matches!(result, HookOutcome::Continue { modified: None }));
}

#[tokio::test]
async fn test_rule_hook_combined_actions() {
    let registry = Arc::new(HookRegistry::new());

    let bundle = HookBundleConfig {
        rules: vec![HookRuleConfig {
            name: "combined".to_string(),
            points: vec![HookPoint::BeforeInbound],
            priority: None,
            failure_mode: None,
            timeout_ms: None,
            when_regex: None,
            reject_reason: None,
            replacements: vec![RegexReplacementConfig {
                pattern: "secret".to_string(),
                replacement: "safe".to_string(),
            }],
            prepend: Some("[".to_string()),
            append: Some("]".to_string()),
        }],
        outbound_webhooks: vec![],
    };

    register_bundle(&registry, "workspace:hooks/hooks.json", bundle).await;
    let result = registry.run(&inbound_event("secret")).await.unwrap();
    match result {
        HookOutcome::Continue {
            modified: Some(value),
        } => assert_eq!(value, "[safe]"),
        other => panic!("expected modified output, got {other:?}"),
    }
}
