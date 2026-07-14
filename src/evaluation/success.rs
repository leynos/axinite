//! Success evaluation for jobs.

use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::context::{ActionRecord, JobContext};
use crate::error::EvaluationError;

/// Result of evaluating job success.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResult {
    /// Whether the job was successful.
    pub success: bool,
    /// Confidence in the evaluation (0-1).
    pub confidence: f64,
    /// Detailed reasoning.
    pub reasoning: String,
    /// Specific issues found.
    pub issues: Vec<String>,
    /// Suggestions for improvement.
    pub suggestions: Vec<String>,
    /// Quality score (0-100).
    pub quality_score: u32,
}

impl EvaluationResult {
    /// Create a successful evaluation.
    pub fn success(reasoning: impl Into<String>, quality_score: u32) -> Self {
        Self {
            success: true,
            confidence: 0.9,
            reasoning: reasoning.into(),
            issues: vec![],
            suggestions: vec![],
            quality_score,
        }
    }

    /// Create a failed evaluation.
    pub fn failure(reasoning: impl Into<String>, issues: Vec<String>) -> Self {
        Self {
            success: false,
            confidence: 0.9,
            reasoning: reasoning.into(),
            issues,
            suggestions: vec![],
            quality_score: 0,
        }
    }
}

/// Trait for success evaluators.
pub trait SuccessEvaluator: Send + Sync {
    /// Use an explicit future type so the public trait keeps a `Send`
    /// contract without relying on the `async-trait` proc macro.
    /// Evaluate whether a job was completed successfully.
    fn evaluate(
        &self,
        job: &JobContext,
        actions: &[ActionRecord],
        output: Option<&str>,
    ) -> impl Future<Output = Result<EvaluationResult, EvaluationError>> + Send;
}

#[cfg(test)]
mod tests;
