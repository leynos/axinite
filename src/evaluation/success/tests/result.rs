//! Tests for `EvaluationResult` construction and serde round-tripping.

use crate::evaluation::EvaluationResult;

#[test]
fn test_evaluation_result_success_defaults() {
    let result = EvaluationResult::success("all good", 85);
    assert!(result.success);
    assert_eq!(result.confidence, 0.9);
    assert_eq!(result.reasoning, "all good");
    assert!(result.issues.is_empty());
    assert!(result.suggestions.is_empty());
    assert_eq!(result.quality_score, 85);
}

#[test]
fn test_evaluation_result_failure_defaults() {
    let issues = vec!["bad thing".to_string(), "worse thing".to_string()];
    let result = EvaluationResult::failure("went wrong", issues.clone());
    assert!(!result.success);
    assert_eq!(result.confidence, 0.9);
    assert_eq!(result.reasoning, "went wrong");
    assert_eq!(result.issues, issues);
    assert_eq!(result.quality_score, 0);
}

#[test]
fn test_evaluation_result_serde_roundtrip() {
    let result = EvaluationResult {
        success: true,
        confidence: 0.75,
        reasoning: "looks fine".to_string(),
        issues: vec!["minor".to_string()],
        suggestions: vec!["try harder".to_string()],
        quality_score: 60,
    };
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: EvaluationResult = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.success, result.success);
    assert_eq!(deserialized.confidence, result.confidence);
    assert_eq!(deserialized.reasoning, result.reasoning);
    assert_eq!(deserialized.issues, result.issues);
    assert_eq!(deserialized.suggestions, result.suggestions);
    assert_eq!(deserialized.quality_score, result.quality_score);
}
