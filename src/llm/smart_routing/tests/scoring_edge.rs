//! Scorer tests for multi-dimensional boost, explicit tier hints, custom
//! domain keywords, and edge cases.

use crate::llm::smart_routing::{
    ScorerConfig, ScorerWeights, Tier, score_complexity, score_complexity_with_config,
};

// -----------------------------------------------------------------------
// Score complexity: multi-dimensional boost
// -----------------------------------------------------------------------

#[test]
fn score_multi_dimensional_boost() {
    // This triggers reasoning, multi-step, code, domain, creativity, safety
    let result = score_complexity(
        "First, explain why the kubernetes deployment fails. \
         Then refactor the auth module to fix the vulnerability. \
         After that, write a security report comparing the approaches.",
    );
    assert!(
        result.hints.iter().any(|h| h.contains("Multi-dimensional")),
        "Expected multi-dimensional boost, hints: {:?}",
        result.hints
    );
}

// -----------------------------------------------------------------------
// Score complexity: explicit tier hint
// -----------------------------------------------------------------------

#[test]
fn score_explicit_tier_hint_flash() {
    let result = score_complexity("[tier:flash] This looks complex but override to flash");
    assert_eq!(result.tier, Tier::Flash);
    assert!(
        result
            .hints
            .iter()
            .any(|h| h.contains("Explicit tier hint"))
    );
}

#[test]
fn score_explicit_tier_hint_frontier() {
    let result = score_complexity("[tier:frontier] Simple question but I want the best");
    assert_eq!(result.tier, Tier::Frontier);
}

#[test]
fn score_explicit_tier_hint_case_insensitive() {
    let result = score_complexity("[tier:PRO] some message");
    assert_eq!(result.tier, Tier::Pro);
}

// -----------------------------------------------------------------------
// Score complexity: custom domain keywords
// -----------------------------------------------------------------------

#[test]
fn score_custom_domain_keywords_override_defaults() {
    // Default keywords should match "kubernetes"
    let default_result = score_complexity("How do I deploy kubernetes?");
    let default_domain = default_result
        .components
        .get("domain_specific")
        .copied()
        .unwrap_or(0);
    assert!(
        default_domain > 0,
        "Default keywords should match 'kubernetes'"
    );

    // Custom keywords that DON'T include kubernetes
    let config = ScorerConfig {
        weights: ScorerWeights::default(),
        domain_keywords: Some(vec!["mycompany".to_string(), "myproduct".to_string()]),
    };
    let custom_result = score_complexity_with_config("How do I deploy kubernetes?", &config);
    let custom_domain = custom_result
        .components
        .get("domain_specific")
        .copied()
        .unwrap_or(0);
    assert_eq!(
        custom_domain, 0,
        "Custom keywords shouldn't match 'kubernetes'"
    );

    // Custom keywords should match their own terms
    let custom_result2 = score_complexity_with_config("Tell me about myproduct features", &config);
    let custom_domain2 = custom_result2
        .components
        .get("domain_specific")
        .copied()
        .unwrap_or(0);
    assert!(
        custom_domain2 > 0,
        "Custom keywords should match 'myproduct'"
    );
}

// -----------------------------------------------------------------------
// Score complexity: edge cases
// -----------------------------------------------------------------------

#[test]
fn score_whitespace_only_is_flash() {
    let result = score_complexity("   \n\t  ");
    assert_eq!(result.tier, Tier::Flash);
}

#[test]
fn score_single_word_no_keywords() {
    let result = score_complexity("banana");
    assert!(
        result.tier == Tier::Flash || result.tier == Tier::Standard,
        "Single non-keyword word should be Flash or Standard, got {:?}",
        result.tier
    );
}

#[test]
fn score_very_long_prompt_is_at_least_standard() {
    let long = "Tell me about ".to_string() + &"things ".repeat(200);
    let result = score_complexity(&long);
    assert!(
        result.total >= 16,
        "Very long prompt should score at least Standard, got {}",
        result.total
    );
}

#[test]
fn score_all_dimensions_have_entries() {
    let result =
        score_complexity("First, explain why the function fails. Then write a fix and deploy it.");
    let expected_keys = [
        "reasoning_words",
        "token_estimate",
        "code_indicators",
        "multi_step",
        "domain_specific",
        "ambiguity",
        "creativity",
        "precision",
        "context_dependency",
        "tool_likelihood",
        "safety_sensitivity",
        "question_complexity",
        "sentence_complexity",
    ];
    for key in &expected_keys {
        assert!(
            result.components.contains_key(*key),
            "Missing component: {key}"
        );
    }
}

#[test]
fn score_is_clamped_to_100() {
    // Trigger every dimension hard
    let prompt = "First, explain why the kubernetes docker terraform deployment on aws fails. \
         Then analyze the security vulnerability and compare the trade-offs. \
         After that, write a detailed blog post report with code examples: \
         ```rust\nfn main() {}\n``` \
         Calculate exactly how many steps are needed? Why? How? \
         Deploy to production mainnet. Review the authentication token password.";
    let result = score_complexity(prompt);
    assert!(
        result.total <= 100,
        "Score should be clamped to 100, got {}",
        result.total
    );
}
