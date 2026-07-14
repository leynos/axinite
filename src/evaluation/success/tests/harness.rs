//! Test-only rule-based evaluator and shared action/job helpers.

use crate::context::{ActionRecord, JobContext};
use crate::error::EvaluationError;
use crate::evaluation::{EvaluationResult, SuccessEvaluator};

/// Rule-based success evaluator (test-only; no production callers).
pub(super) struct RuleBasedEvaluator {
    pub(super) min_action_success_rate: f64,
    pub(super) max_failures: u32,
}

impl RuleBasedEvaluator {
    pub(super) fn new() -> Self {
        Self {
            min_action_success_rate: 0.8,
            max_failures: 3,
        }
    }

    pub(super) fn with_min_success_rate(mut self, rate: f64) -> Self {
        self.min_action_success_rate = rate;
        self
    }

    pub(super) fn with_max_failures(mut self, max: u32) -> Self {
        self.max_failures = max;
        self
    }
}

impl Default for RuleBasedEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether an action's error text signals a critical or fatal failure.
fn is_critical_error(error: &str) -> bool {
    let lowered = error.to_lowercase();
    lowered.contains("critical") || lowered.contains("fatal")
}

impl SuccessEvaluator for RuleBasedEvaluator {
    async fn evaluate(
        &self,
        job: &JobContext,
        actions: &[ActionRecord],
        _output: Option<&str>,
    ) -> Result<EvaluationResult, EvaluationError> {
        let mut issues = Vec::new();

        if actions.is_empty() {
            return Ok(EvaluationResult::failure(
                "No actions were taken",
                vec!["No actions recorded".to_string()],
            ));
        }

        let successful = actions.iter().filter(|a| a.success).count();
        let total = actions.len();
        let success_rate = successful as f64 / total as f64;

        if success_rate < self.min_action_success_rate {
            issues.push(format!(
                "Action success rate {:.1}% below threshold {:.1}%",
                success_rate * 100.0,
                self.min_action_success_rate * 100.0
            ));
        }

        let failures = actions.iter().filter(|a| !a.success).count() as u32;
        if failures > self.max_failures {
            issues.push(format!(
                "Too many failures: {} (max {})",
                failures, self.max_failures
            ));
        }

        for action in actions.iter().filter(|a| !a.success) {
            if let Some(ref error) = action.error
                && is_critical_error(error)
            {
                issues.push(format!("Critical error in {}: {}", action.tool_name, error));
            }
        }

        if job.state != crate::context::JobState::Completed
            && job.state != crate::context::JobState::Submitted
        {
            issues.push(format!("Job not in completed state: {:?}", job.state));
        }

        let quality_score = if issues.is_empty() {
            let base_score = (success_rate * 80.0) as u32;
            let completion_bonus = if job.state == crate::context::JobState::Completed {
                20
            } else {
                0
            };
            (base_score + completion_bonus).min(100)
        } else {
            ((success_rate * 50.0) as u32).min(50)
        };

        if issues.is_empty() {
            Ok(EvaluationResult::success(
                format!(
                    "Job completed successfully with {}/{} actions succeeding ({:.1}%)",
                    successful,
                    total,
                    success_rate * 100.0
                ),
                quality_score,
            ))
        } else {
            Ok(EvaluationResult {
                success: false,
                confidence: 0.85,
                reasoning: format!("Job had {} issues", issues.len()),
                issues,
                suggestions: vec![
                    "Review failed actions for common patterns".to_string(),
                    "Consider adjusting retry logic".to_string(),
                ],
                quality_score,
            })
        }
    }
}

pub(super) fn create_action(success: bool) -> ActionRecord {
    create_action_with_error(success, "Test error")
}

pub(super) fn create_action_with_error(success: bool, error_msg: &str) -> ActionRecord {
    let mut action = ActionRecord::new(0, "test", serde_json::json!({}));
    if success {
        action = action.succeed(
            None,
            serde_json::json!({}),
            std::time::Duration::from_secs(1),
        );
    } else {
        action = action.fail(error_msg, std::time::Duration::from_secs(1));
    }
    action
}

pub(super) fn completed_job(title: &str) -> JobContext {
    let mut job = JobContext::new(title, "test job");
    job.transition_to(crate::context::JobState::InProgress, None)
        .unwrap();
    job.transition_to(crate::context::JobState::Completed, None)
        .unwrap();
    job
}

// --- RuleBasedEvaluator builder ---

#[test]
fn test_rule_based_evaluator_default() {
    let eval = RuleBasedEvaluator::default();
    assert_eq!(eval.min_action_success_rate, 0.8);
    assert_eq!(eval.max_failures, 3);
}

#[test]
fn test_rule_based_evaluator_builder_methods() {
    let eval = RuleBasedEvaluator::new()
        .with_min_success_rate(0.5)
        .with_max_failures(10);
    assert_eq!(eval.min_action_success_rate, 0.5);
    assert_eq!(eval.max_failures, 10);
}
