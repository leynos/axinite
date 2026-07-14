//! Behavioural tests for `RuleBasedEvaluator::evaluate`.

use crate::context::JobContext;
use crate::evaluation::SuccessEvaluator;

use super::harness::{RuleBasedEvaluator, completed_job, create_action, create_action_with_error};

#[tokio::test]
async fn test_rule_based_evaluator_success() {
    let evaluator = RuleBasedEvaluator::new();

    let mut job = JobContext::new("Test", "Test job");
    job.transition_to(crate::context::JobState::InProgress, None)
        .unwrap();
    job.transition_to(crate::context::JobState::Completed, None)
        .unwrap();

    let actions = vec![
        create_action(true),
        create_action(true),
        create_action(true),
    ];

    let result = evaluator.evaluate(&job, &actions, None).await.unwrap();
    assert!(result.success);
    assert!(result.quality_score > 80);
}

#[tokio::test]
async fn test_rule_based_evaluator_failure() {
    let evaluator = RuleBasedEvaluator::new().with_max_failures(1);

    let job = JobContext::new("Test", "Test job");

    let actions = vec![
        create_action(true),
        create_action(false),
        create_action(false),
    ];

    let result = evaluator.evaluate(&job, &actions, None).await.unwrap();
    assert!(!result.success);
    assert!(!result.issues.is_empty());
}

#[tokio::test]
async fn test_empty_actions_fails() {
    let eval = RuleBasedEvaluator::new();
    let job = completed_job("empty");
    let result = eval.evaluate(&job, &[], None).await.unwrap();
    assert!(!result.success);
    assert!(result.issues.iter().any(|i| i.contains("No actions")));
}

#[tokio::test]
async fn test_all_actions_succeed_completed_job_gets_100() {
    let eval = RuleBasedEvaluator::new();
    let job = completed_job("perfect");
    let actions = vec![
        create_action(true),
        create_action(true),
        create_action(true),
        create_action(true),
        create_action(true),
    ];
    let result = eval.evaluate(&job, &actions, None).await.unwrap();
    assert!(result.success);
    // 100% success rate -> base 80, completion bonus 20 -> 100
    assert_eq!(result.quality_score, 100);
}

#[tokio::test]
async fn test_quality_score_no_completion_bonus_for_pending_job() {
    // Even if all actions succeed, a non-completed job gets flagged
    let eval = RuleBasedEvaluator::new();
    let job = JobContext::new("pending", "still pending");
    let actions = vec![create_action(true)];
    let result = eval.evaluate(&job, &actions, None).await.unwrap();
    // Job not in completed state => issues present
    assert!(!result.success);
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.contains("not in completed state"))
    );
}

#[tokio::test]
async fn test_submitted_state_counts_as_completed() {
    let eval = RuleBasedEvaluator::new();
    let mut job = JobContext::new("submitted", "test");
    job.transition_to(crate::context::JobState::InProgress, None)
        .unwrap();
    job.transition_to(crate::context::JobState::Completed, None)
        .unwrap();
    job.transition_to(crate::context::JobState::Submitted, None)
        .unwrap();
    let actions = vec![create_action(true)];
    let result = eval.evaluate(&job, &actions, None).await.unwrap();
    // Submitted is treated like completed for state check (no issue),
    // but completion bonus only applies for Completed state
    assert!(result.success);
}

#[tokio::test]
async fn test_success_rate_below_threshold_fails() {
    let eval = RuleBasedEvaluator::new().with_min_success_rate(0.9);
    let job = completed_job("threshold");
    // 4 out of 5 = 80%, below 90% threshold
    let actions = vec![
        create_action(true),
        create_action(true),
        create_action(true),
        create_action(true),
        create_action(false),
    ];
    let result = eval.evaluate(&job, &actions, None).await.unwrap();
    assert!(!result.success);
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.contains("success rate") && i.contains("below threshold"))
    );
}

#[tokio::test]
async fn test_too_many_failures_flagged() {
    let eval = RuleBasedEvaluator::new().with_max_failures(1);
    let job = completed_job("failures");
    // 8 successes, 2 failures: rate is 80% (passes default 0.8) but failures > max 1
    let actions = vec![
        create_action(true),
        create_action(true),
        create_action(true),
        create_action(true),
        create_action(true),
        create_action(true),
        create_action(true),
        create_action(true),
        create_action(false),
        create_action(false),
    ];
    let result = eval.evaluate(&job, &actions, None).await.unwrap();
    assert!(!result.success);
    assert!(
        result
            .issues
            .iter()
            .any(|i| i.contains("Too many failures"))
    );
}

#[tokio::test]
async fn test_critical_error_detected() {
    let eval = RuleBasedEvaluator::new().with_max_failures(10);
    let job = completed_job("critical");
    let actions = vec![
        create_action(true),
        create_action(true),
        create_action(true),
        create_action(true),
        create_action_with_error(false, "A CRITICAL system failure occurred"),
    ];
    let result = eval.evaluate(&job, &actions, None).await.unwrap();
    assert!(!result.success);
    assert!(result.issues.iter().any(|i| i.contains("Critical error")));
}

#[tokio::test]
async fn test_fatal_error_detected() {
    let eval = RuleBasedEvaluator::new().with_max_failures(10);
    let job = completed_job("fatal");
    let actions = vec![
        create_action(true),
        create_action(true),
        create_action(true),
        create_action(true),
        create_action_with_error(false, "Fatal: disk full"),
    ];
    let result = eval.evaluate(&job, &actions, None).await.unwrap();
    assert!(result.issues.iter().any(|i| i.contains("Critical error")));
}

#[tokio::test]
async fn test_quality_score_capped_at_50_with_issues() {
    let eval = RuleBasedEvaluator::new()
        .with_min_success_rate(0.0)
        .with_max_failures(100);
    // Job not completed => issues present, quality capped
    let job = JobContext::new("capped", "test");
    let actions = vec![create_action(true)];
    let result = eval.evaluate(&job, &actions, None).await.unwrap();
    assert!(!result.success);
    assert!(result.quality_score <= 50);
}

#[tokio::test]
async fn test_failed_result_includes_suggestions() {
    let eval = RuleBasedEvaluator::new().with_max_failures(0);
    let job = completed_job("suggestions");
    let actions = vec![create_action(false)];
    let result = eval.evaluate(&job, &actions, None).await.unwrap();
    assert!(!result.success);
    assert!(!result.suggestions.is_empty());
    assert_eq!(result.confidence, 0.85);
}

#[tokio::test]
async fn test_single_successful_action_completed_job() {
    let eval = RuleBasedEvaluator::new();
    let job = completed_job("single");
    let actions = vec![create_action(true)];
    let result = eval.evaluate(&job, &actions, None).await.unwrap();
    assert!(result.success);
    // 100% rate -> base 80, + 20 completion = 100
    assert_eq!(result.quality_score, 100);
    assert!(result.reasoning.contains("1/1"));
}
