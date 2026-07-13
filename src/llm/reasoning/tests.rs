//! Unit tests for reasoning plan extraction and structural helpers.
//!
//! Tests are grouped into themed submodules; shared utility tests live here.

mod code_aware;
mod integration;
mod intent_detection;
mod prompts;
mod recovery_tests;
mod stripping;
mod truncation_tests;

use super::cleaning::QUICK_TAG_RE;
use super::planning::extract_json;
use super::*;
use crate::llm::{ChatMessage, CompletionRequest, ToolDefinition};

// ---- Utility / structural tests ----

#[test]
fn static_tag_regexes_compile() {
    assert!(QUICK_TAG_RE.is_some(), "QUICK_TAG_RE must compile");
    assert!(THINKING_TAG_RE.is_some(), "THINKING_TAG_RE must compile");
    assert!(FINAL_TAG_RE.is_some(), "FINAL_TAG_RE must compile");
    assert!(
        PIPE_REASONING_TAG_RE.is_some(),
        "PIPE_REASONING_TAG_RE must compile"
    );
}

#[test]
fn test_extract_json() {
    let text = r#"Here's the plan:
{"goal": "test", "actions": []}
That's my plan."#;
    let json = extract_json(text).unwrap();
    assert!(json.starts_with('{'));
    assert!(json.ends_with('}'));
}

#[test]
fn test_reasoning_context_builder() {
    let context = ReasoningContext::new()
        .with_message(ChatMessage::user("Hello"))
        .with_job("Test job");
    assert_eq!(context.messages.len(), 1);
    assert!(context.job_description.is_some());
}

// ---- plan/evaluate bypass clean_response (Bug #564-2) ----

#[test]
fn test_clean_response_strips_think_before_json_plan() {
    let raw = r#"<think>I need to plan the steps carefully...</think>{"steps": [{"description": "Step 1", "tool": "search", "expected_outcome": "results"}], "reasoning": "Simple plan"}"#;
    let cleaned = clean_response(raw);
    // After cleaning, the JSON should be parseable
    let json_str = extract_json(&cleaned).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(json_str).unwrap();
    assert!(parsed.get("steps").is_some());
}

#[test]
fn test_clean_response_strips_think_before_json_evaluation() {
    let raw = r#"<think>Let me evaluate whether this was successful...</think>{"success": true, "confidence": 0.95, "reasoning": "Task completed", "issues": [], "suggestions": []}"#;
    let cleaned = clean_response(raw);
    let json_str = extract_json(&cleaned).unwrap();
    let eval: SuccessEvaluation = serde_json::from_str(json_str).unwrap();
    assert!(eval.success);
    assert_eq!(eval.confidence, 0.95);
}
