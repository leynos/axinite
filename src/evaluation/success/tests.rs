//! Unit tests for success evaluation using a rule-based evaluator.
//!
//! - [`harness`] — the test-only `RuleBasedEvaluator` and action/job helpers
//! - [`result`] — `EvaluationResult` construction and serde tests
//! - [`evaluate`] — behavioural tests for `RuleBasedEvaluator::evaluate`

mod evaluate;
mod harness;
mod result;
