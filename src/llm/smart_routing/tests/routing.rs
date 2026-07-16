//! Tests for provider-level routing, cascade escalation, and stats.

use std::sync::Arc;

use crate::llm::ChatMessage;
use crate::llm::provider::{CompletionRequest, LlmProvider, ToolCompletionRequest};
use crate::llm::smart_routing::{SmartRoutingConfig, SmartRoutingProvider};
use crate::testing::StubLlm;

use super::default_config;

// -----------------------------------------------------------------------
// Provider routing tests
// -----------------------------------------------------------------------

fn make_request(content: &str) -> CompletionRequest {
    CompletionRequest::new(vec![ChatMessage::user(content)])
}

fn make_tool_request() -> ToolCompletionRequest {
    ToolCompletionRequest::new(vec![ChatMessage::user("implement a search")], vec![])
}

#[tokio::test]
async fn simple_task_routes_to_cheap() {
    let primary = Arc::new(StubLlm::new("primary-response").with_model_name("primary"));
    let cheap = Arc::new(StubLlm::new("cheap-response").with_model_name("cheap"));

    let router = SmartRoutingProvider::new(
        primary.clone(),
        cheap.clone(),
        SmartRoutingConfig {
            cascade_enabled: false,
            ..default_config()
        },
    );

    let resp = router.complete(make_request("hello")).await.unwrap();
    assert_eq!(resp.content, "cheap-response");
    assert_eq!(cheap.calls(), 1);
    assert_eq!(primary.calls(), 0);
}

#[tokio::test]
async fn complex_task_routes_to_primary() {
    let primary = Arc::new(StubLlm::new("primary-response").with_model_name("primary"));
    let cheap = Arc::new(StubLlm::new("cheap-response").with_model_name("cheap"));

    let router = SmartRoutingProvider::new(primary.clone(), cheap.clone(), default_config());

    // Security audit triggers Frontier via pattern override → Complex → primary
    let resp = router
        .complete(make_request(
            "Please do a security audit of this smart contract",
        ))
        .await
        .unwrap();
    assert_eq!(resp.content, "primary-response");
    assert_eq!(primary.calls(), 1);
    assert_eq!(cheap.calls(), 0);
}

#[tokio::test]
async fn tool_use_always_routes_to_primary() {
    let primary = Arc::new(StubLlm::new("primary-response").with_model_name("primary"));
    let cheap = Arc::new(StubLlm::new("cheap-response").with_model_name("cheap"));

    let router = SmartRoutingProvider::new(primary.clone(), cheap.clone(), default_config());

    let resp = router
        .complete_with_tools(make_tool_request())
        .await
        .unwrap();
    assert_eq!(resp.content, Some("primary-response".to_string()));
    assert_eq!(primary.calls(), 1);
    assert_eq!(cheap.calls(), 0);
}

#[tokio::test]
async fn stats_increment_correctly() {
    let primary = Arc::new(StubLlm::new("primary").with_model_name("primary"));
    let cheap = Arc::new(StubLlm::new("cheap").with_model_name("cheap"));

    let router = SmartRoutingProvider::new(
        primary,
        cheap,
        SmartRoutingConfig {
            cascade_enabled: false,
            ..default_config()
        },
    );

    // Simple → cheap (greeting pattern override)
    router.complete(make_request("hello")).await.unwrap();
    // Complex → primary (security audit pattern override → Frontier)
    router
        .complete(make_request("security audit review"))
        .await
        .unwrap();
    // Tool use → primary
    router
        .complete_with_tools(make_tool_request())
        .await
        .unwrap();

    let stats = router.stats();
    assert_eq!(stats.total_requests, 3);
    assert_eq!(stats.cheap_requests, 1);
    assert_eq!(stats.primary_requests, 2);
    assert_eq!(stats.cascade_escalations, 0);
}

#[tokio::test]
async fn cascade_escalates_on_uncertain_response() {
    let primary = Arc::new(StubLlm::new("primary-response").with_model_name("primary"));
    let cheap = Arc::new(StubLlm::new("I'm not sure about that.").with_model_name("cheap"));

    let router = SmartRoutingProvider::new(
        primary.clone(),
        cheap.clone(),
        SmartRoutingConfig {
            cascade_enabled: true,
            ..default_config()
        },
    );

    // A Pro-tier task (triggers Moderate → cascade)
    let resp = router
        .complete(make_request("Deploy this to production"))
        .await
        .unwrap();

    // Should have escalated to primary
    assert_eq!(resp.content, "primary-response");
    assert_eq!(cheap.calls(), 1);
    assert_eq!(primary.calls(), 1);

    let stats = router.stats();
    assert_eq!(stats.cascade_escalations, 1);
}

#[tokio::test]
async fn cascade_does_not_escalate_on_confident_response() {
    let primary = Arc::new(StubLlm::new("primary-response").with_model_name("primary"));
    let cheap = Arc::new(
        StubLlm::new("Deployed successfully to production mainnet.").with_model_name("cheap"),
    );

    let router = SmartRoutingProvider::new(
        primary.clone(),
        cheap.clone(),
        SmartRoutingConfig {
            cascade_enabled: true,
            ..default_config()
        },
    );

    let resp = router
        .complete(make_request("Deploy this to production"))
        .await
        .unwrap();

    // Should NOT have escalated
    assert!(resp.content.contains("Deployed successfully"));
    assert_eq!(cheap.calls(), 1);
    assert_eq!(primary.calls(), 0);

    let stats = router.stats();
    assert_eq!(stats.cascade_escalations, 0);
}

#[tokio::test]
async fn model_name_returns_primary() {
    let primary = Arc::new(StubLlm::new("ok").with_model_name("sonnet"));
    let cheap = Arc::new(StubLlm::new("ok").with_model_name("haiku"));

    let router = SmartRoutingProvider::new(primary, cheap, default_config());
    assert_eq!(router.model_name(), "sonnet");
    assert_eq!(router.active_model_name(), "sonnet");
}

#[tokio::test]
async fn tier_hint_overrides_pattern_override() {
    // "[tier:flash] security audit review" has both a Flash tier hint and
    // a Frontier pattern override. Tier hints should win.
    let primary = Arc::new(StubLlm::new("primary").with_model_name("primary"));
    let cheap = Arc::new(StubLlm::new("cheap").with_model_name("cheap"));

    let router = SmartRoutingProvider::new(
        primary.clone(),
        cheap.clone(),
        SmartRoutingConfig {
            cascade_enabled: false,
            ..default_config()
        },
    );

    router
        .complete(make_request("[tier:flash] security audit review"))
        .await
        .unwrap();

    // Tier hint → Flash → Simple → cheap model
    assert_eq!(cheap.calls(), 1);
    assert_eq!(primary.calls(), 0);
}

#[tokio::test]
async fn trimmed_greeting_matches_override() {
    // Trailing whitespace should not prevent the greeting override from matching.
    let primary = Arc::new(StubLlm::new("primary").with_model_name("primary"));
    let cheap = Arc::new(StubLlm::new("cheap").with_model_name("cheap"));

    let router = SmartRoutingProvider::new(
        primary.clone(),
        cheap.clone(),
        SmartRoutingConfig {
            cascade_enabled: false,
            ..default_config()
        },
    );

    router.complete(make_request("  hello  \n")).await.unwrap();

    // Should match greeting override → Flash → Simple → cheap model
    assert_eq!(cheap.calls(), 1);
    assert_eq!(primary.calls(), 0);
}
