//! Scorer tests for tier boundaries and individual scoring dimensions.

use crate::llm::smart_routing::{Tier, score_complexity};

// -----------------------------------------------------------------------
// Score complexity: tier boundaries
// -----------------------------------------------------------------------

#[test]
fn score_empty_prompt_is_flash() {
    let result = score_complexity("");
    assert_eq!(result.tier, Tier::Flash);
    assert!(result.total <= 15);
}

#[test]
fn score_simple_greeting_is_flash() {
    let result = score_complexity("Hi");
    assert_eq!(result.tier, Tier::Flash);
    assert!(result.total <= 15);
}

#[test]
fn score_quick_question_is_flash_or_standard() {
    let result = score_complexity("What time is it?");
    assert!(
        result.tier == Tier::Flash || result.tier == Tier::Standard,
        "Expected Flash or Standard, got {:?} (score {})",
        result.tier,
        result.total
    );
}

#[test]
fn score_code_task_is_standard_or_higher() {
    let result = score_complexity("Implement a function to sort an array in TypeScript");
    assert!(
        result.tier == Tier::Standard || result.tier == Tier::Pro,
        "Expected Standard or Pro, got {:?} (score {})",
        result.tier,
        result.total
    );
}

#[test]
fn score_complex_analysis_is_at_least_standard() {
    let result = score_complexity(
        "Explain why React uses a virtual DOM and compare it to Svelte's approach. \
         Consider the trade-offs for performance and developer experience.",
    );
    assert!(
        result.total >= 20,
        "Expected score >= 20, got {}",
        result.total
    );
    assert!(
        result.tier == Tier::Standard || result.tier == Tier::Pro,
        "Expected Standard or Pro, got {:?}",
        result.tier
    );
}

#[test]
fn score_security_audit_prompt_is_at_least_standard() {
    let result = score_complexity(
        "Analyse this Solidity contract for reentrancy vulnerabilities, \
         check for authentication bypass, and provide a security audit report.",
    );
    assert!(
        result.total >= 16,
        "Expected score >= 16, got {}",
        result.total
    );
}

// -----------------------------------------------------------------------
// Score complexity: individual dimensions
// -----------------------------------------------------------------------

#[test]
fn score_reasoning_dimension() {
    let result = score_complexity("Why is this better? Explain the trade-offs and compare");
    let reasoning = result
        .components
        .get("reasoning_words")
        .copied()
        .unwrap_or(0);
    assert!(
        reasoning >= 100,
        "Expected reasoning >= 100, got {reasoning}"
    );
}

#[test]
fn score_multi_step_dimension() {
    let result = score_complexity(
        "First, read the file at src/auth.ts. Then analyse it for security issues. \
         After that, write a detailed report.",
    );
    let multi_step = result.components.get("multi_step").copied().unwrap_or(0);
    assert!(
        multi_step >= 100,
        "Expected multi_step >= 100, got {multi_step}"
    );
    assert!(result.hints.iter().any(|h| h.contains("multi_step")));
}

#[test]
fn score_code_dimension() {
    let result = score_complexity("Fix the bug in the async function, refactor the module");
    let code = result
        .components
        .get("code_indicators")
        .copied()
        .unwrap_or(0);
    assert!(code >= 50, "Expected code_indicators >= 50, got {code}");
}

#[test]
fn score_safety_dimension() {
    let result = score_complexity("Store the password and encrypt the auth token");
    let safety = result
        .components
        .get("safety_sensitivity")
        .copied()
        .unwrap_or(0);
    assert!(safety >= 100, "Expected safety >= 100, got {safety}");
}

#[test]
fn score_domain_dimension() {
    let result = score_complexity("Deploy the kubernetes cluster on aws with terraform");
    let domain = result
        .components
        .get("domain_specific")
        .copied()
        .unwrap_or(0);
    assert!(
        domain >= 100,
        "Expected domain_specific >= 100, got {domain}"
    );
}

#[test]
fn score_creativity_dimension() {
    let result = score_complexity("Write a blog post about design patterns, then summarize");
    let creativity = result.components.get("creativity").copied().unwrap_or(0);
    assert!(
        creativity >= 100,
        "Expected creativity >= 100, got {creativity}"
    );
}

#[test]
fn score_question_complexity_dimension() {
    let result = score_complexity("Why does this fail? How can I fix it? What if I try X?");
    let qc = result
        .components
        .get("question_complexity")
        .copied()
        .unwrap_or(0);
    assert!(qc >= 60, "Expected question_complexity >= 60, got {qc}");
    assert!(
        result
            .hints
            .iter()
            .any(|h| h.contains("Multiple questions"))
    );
}

#[test]
fn score_sentence_complexity_dimension() {
    let result = score_complexity(
        "This is complex, because it has commas, and conjunctions, \
         however it also has semicolons; moreover, it keeps going, and going",
    );
    let sc = result
        .components
        .get("sentence_complexity")
        .copied()
        .unwrap_or(0);
    assert!(sc >= 60, "Expected sentence_complexity >= 60, got {sc}");
}

#[test]
fn score_token_estimate_for_long_prompt() {
    let long_prompt = "a ".repeat(300); // 600 chars
    let result = score_complexity(&long_prompt);
    let token = result
        .components
        .get("token_estimate")
        .copied()
        .unwrap_or(0);
    assert!(token >= 80, "Expected token_estimate >= 80, got {token}");
}

#[test]
fn score_token_estimate_for_short_prompt() {
    let result = score_complexity("hi");
    let token = result
        .components
        .get("token_estimate")
        .copied()
        .unwrap_or(0);
    assert_eq!(token, 0, "Expected token_estimate == 0, got {token}");
}
