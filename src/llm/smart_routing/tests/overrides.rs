//! Tests for pattern overrides, tier-to-complexity mapping, and
//! uncertainty detection in cheap-model responses.

use std::sync::Arc;

use crate::llm::ChatMessage;
use crate::llm::provider::{CompletionRequest, CompletionResponse};
use crate::llm::smart_routing::patterns::DEFAULT_OVERRIDES;
use crate::llm::smart_routing::{
    ScorerConfig, SmartRoutingProvider, TaskComplexity, Tier, score_complexity_with_config,
};
use crate::testing::StubLlm;

use super::default_config;

// -----------------------------------------------------------------------
// Pattern overrides
// -----------------------------------------------------------------------

#[test]
fn pattern_override_greeting_is_simple() {
    let primary = Arc::new(StubLlm::new("p").with_model_name("primary"));
    let cheap = Arc::new(StubLlm::new("c").with_model_name("cheap"));
    let provider = SmartRoutingProvider::new(primary, cheap, default_config());

    let req = CompletionRequest::new(vec![ChatMessage::user("Hi")]);
    let complexity = provider.classify(&req);
    assert_eq!(complexity, TaskComplexity::Simple);
}

#[test]
fn pattern_override_security_audit_is_complex() {
    let primary = Arc::new(StubLlm::new("p").with_model_name("primary"));
    let cheap = Arc::new(StubLlm::new("c").with_model_name("cheap"));
    let provider = SmartRoutingProvider::new(primary, cheap, default_config());

    let req = CompletionRequest::new(vec![ChatMessage::user(
        "Please do a security audit of this contract",
    )]);
    let complexity = provider.classify(&req);
    assert_eq!(complexity, TaskComplexity::Complex);
}

#[test]
fn pattern_override_production_deploy_is_moderate() {
    let primary = Arc::new(StubLlm::new("p").with_model_name("primary"));
    let cheap = Arc::new(StubLlm::new("c").with_model_name("cheap"));
    let provider = SmartRoutingProvider::new(primary, cheap, default_config());

    let req = CompletionRequest::new(vec![ChatMessage::user("Deploy this to production")]);
    let complexity = provider.classify(&req);
    assert_eq!(complexity, TaskComplexity::Moderate);
}

#[test]
fn pattern_override_time_question_is_simple() {
    let primary = Arc::new(StubLlm::new("p").with_model_name("primary"));
    let cheap = Arc::new(StubLlm::new("c").with_model_name("cheap"));
    let provider = SmartRoutingProvider::new(primary, cheap, default_config());

    let req = CompletionRequest::new(vec![ChatMessage::user("What time is it?")]);
    let complexity = provider.classify(&req);
    assert_eq!(complexity, TaskComplexity::Simple);
}

#[test]
fn pattern_override_time_does_not_match_complex_questions() {
    // The quick-lookup override regex should NOT match "What time complexity..."
    // because it's end-anchored. Verify the regex itself doesn't fire.
    let overrides = &*DEFAULT_OVERRIDES;
    let lookup_override = overrides
        .iter()
        .find(|po| po.tier == Tier::Flash && po.regex.as_str().contains("time"))
        .expect("time lookup override exists");

    assert!(
        !lookup_override
            .regex
            .is_match("What time complexity is merge sort?"),
        "Time override should not match 'What time complexity is merge sort?'"
    );
    // But it should still match actual time lookups
    assert!(lookup_override.regex.is_match("What time is it?"));
    assert!(lookup_override.regex.is_match("what's the date today?"));
}

#[test]
fn empty_domain_keywords_uses_defaults() {
    // An empty custom keywords list should fall back to defaults, not produce
    // a broken regex that matches empty strings everywhere.
    let config = ScorerConfig {
        domain_keywords: Some(vec![]),
        ..ScorerConfig::default()
    };
    let result = score_complexity_with_config("deploy kubernetes to mainnet", &config);
    // Should still detect domain keywords via the default fallback
    assert!(
        result
            .components
            .get("domain_specific")
            .copied()
            .unwrap_or(0)
            > 0,
        "Empty custom keywords should fall back to defaults"
    );
}

// -----------------------------------------------------------------------
// Tier → TaskComplexity mapping
// -----------------------------------------------------------------------

#[test]
fn tier_to_task_complexity_mapping() {
    assert_eq!(TaskComplexity::from(Tier::Flash), TaskComplexity::Simple);
    assert_eq!(TaskComplexity::from(Tier::Standard), TaskComplexity::Simple);
    assert_eq!(TaskComplexity::from(Tier::Pro), TaskComplexity::Moderate);
    assert_eq!(
        TaskComplexity::from(Tier::Frontier),
        TaskComplexity::Complex
    );
}

#[test]
fn tier_from_score_boundaries() {
    assert_eq!(Tier::from_score(0), Tier::Flash);
    assert_eq!(Tier::from_score(15), Tier::Flash);
    assert_eq!(Tier::from_score(16), Tier::Standard);
    assert_eq!(Tier::from_score(40), Tier::Standard);
    assert_eq!(Tier::from_score(41), Tier::Pro);
    assert_eq!(Tier::from_score(65), Tier::Pro);
    assert_eq!(Tier::from_score(66), Tier::Frontier);
    assert_eq!(Tier::from_score(100), Tier::Frontier);
}

#[test]
fn tier_display() {
    assert_eq!(Tier::Flash.as_str(), "flash");
    assert_eq!(Tier::Frontier.to_string(), "frontier");
}

// -----------------------------------------------------------------------
// Uncertainty detection
// -----------------------------------------------------------------------

#[test]
fn detects_uncertain_short_response() {
    let response = CompletionResponse {
        content: "I'm not sure.".to_string(),
        input_tokens: 10,
        output_tokens: 5,
        finish_reason: crate::llm::FinishReason::Stop,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
    };
    assert!(SmartRoutingProvider::response_is_uncertain(&response));
}

#[test]
fn detects_empty_response_as_uncertain() {
    let response = CompletionResponse {
        content: "".to_string(),
        input_tokens: 10,
        output_tokens: 0,
        finish_reason: crate::llm::FinishReason::Stop,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
    };
    assert!(SmartRoutingProvider::response_is_uncertain(&response));
}

#[test]
fn short_confident_response_is_not_uncertain() {
    let response = CompletionResponse {
        content: "Yes.".to_string(),
        input_tokens: 10,
        output_tokens: 1,
        finish_reason: crate::llm::FinishReason::Stop,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
    };
    assert!(!SmartRoutingProvider::response_is_uncertain(&response));
}

#[test]
fn confident_response_is_not_uncertain() {
    let response = CompletionResponse {
        content: "The answer is 42. This is a well-known constant from the Hitchhiker's Guide."
            .to_string(),
        input_tokens: 10,
        output_tokens: 20,
        finish_reason: crate::llm::FinishReason::Stop,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
    };
    assert!(!SmartRoutingProvider::response_is_uncertain(&response));
}
