//! Metrics types for test instrumentation.
//!
//! These types were previously in the `axinite::benchmark::metrics` module.
//! They now live directly in the test support crate to keep benchmark-specific
//! types out of the main library.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Per-scenario metrics
// ---------------------------------------------------------------------------

/// Execution metrics collected from a single scenario run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceMetrics {
    /// Wall-clock time in milliseconds for the entire scenario.
    pub wall_time_ms: u64,
    /// Number of LLM API calls made.
    pub llm_calls: u32,
    /// Total input tokens across all LLM calls.
    pub input_tokens: u32,
    /// Total output tokens across all LLM calls.
    pub output_tokens: u32,
    /// Estimated cost in USD (input + output token costs).
    pub estimated_cost_usd: f64,
    /// Per-tool-call invocation records.
    pub tool_calls: Vec<ToolInvocation>,
    /// Number of agent turns (message send -> response cycles).
    pub turns: u32,
    /// Whether the agent hit its max_tool_iterations limit.
    pub hit_iteration_limit: bool,
    /// Whether the scenario timed out waiting for responses.
    pub hit_timeout: bool,
}

impl TraceMetrics {
    /// Total number of tool invocations.
    pub fn total_tool_calls(&self) -> usize {
        self.tool_calls.len()
    }

    /// Number of tool invocations that failed.
    pub fn failed_tool_calls(&self) -> usize {
        self.tool_calls.iter().filter(|t| !t.success).count()
    }

    /// Total tool execution time in milliseconds.
    pub fn total_tool_time_ms(&self) -> u64 {
        self.tool_calls.iter().map(|t| t.duration_ms).sum()
    }
}

/// A single tool invocation with timing and success status.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    /// Tool name.
    pub name: String,
    /// Execution duration in milliseconds.
    pub duration_ms: u64,
    /// Whether the tool completed successfully.
    pub success: bool,
}

// ---------------------------------------------------------------------------
// Per-turn metrics (multi-turn scenarios)
// ---------------------------------------------------------------------------

/// Per-turn metrics for multi-turn scenarios.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnMetrics {
    pub turn_index: usize,
    pub user_message: String,
    pub wall_time_ms: u64,
    pub llm_calls: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub tool_calls: Vec<ToolInvocation>,
    pub response: String,
    pub assertions_passed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judge_score: Option<u8>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// Scenario result
// ---------------------------------------------------------------------------

/// Result of running a single test scenario.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    /// Unique identifier for this scenario (e.g., test function name).
    pub scenario_id: String,
    /// Whether all assertions passed.
    pub passed: bool,
    /// Execution metrics.
    pub trace: TraceMetrics,
    /// The agent's final response text.
    pub response: String,
    /// Error message if the scenario failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Per-turn metrics for multi-turn scenarios.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub turn_metrics: Vec<TurnMetrics>,
}

// ---------------------------------------------------------------------------
// Run result (aggregate)
// ---------------------------------------------------------------------------

/// Aggregate results across multiple scenario runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    /// Unique run identifier.
    pub run_id: String,
    /// Fraction of scenarios that passed (0.0 - 1.0).
    pub pass_rate: f64,
    /// Total estimated cost across all scenarios.
    pub total_cost_usd: f64,
    /// Total wall-clock time across all scenarios.
    pub total_wall_time_ms: u64,
    /// Individual scenario results.
    pub scenarios: Vec<ScenarioResult>,
    /// Git commit hash for reproducibility.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit_hash: Option<String>,
    /// Number of scenarios skipped (e.g., due to budget cap).
    #[serde(default)]
    pub skipped_scenarios: usize,
}

impl RunResult {
    /// Build a RunResult from a list of scenario results.
    pub fn from_scenarios(run_id: impl Into<String>, scenarios: Vec<ScenarioResult>) -> Self {
        let passed = scenarios.iter().filter(|s| s.passed).count();
        let pass_rate = if scenarios.is_empty() {
            0.0
        } else {
            passed as f64 / scenarios.len() as f64
        };
        let total_cost_usd: f64 = scenarios.iter().map(|s| s.trace.estimated_cost_usd).sum();
        let total_wall_time_ms: u64 = scenarios.iter().map(|s| s.trace.wall_time_ms).sum();

        Self {
            run_id: run_id.into(),
            pass_rate,
            total_cost_usd,
            total_wall_time_ms,
            scenarios,
            commit_hash: None,
            skipped_scenarios: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Baseline comparison
// ---------------------------------------------------------------------------

/// A single metric comparison between baseline and current run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDelta {
    pub scenario_id: String,
    pub metric: String,
    pub baseline: f64,
    pub current: f64,
    pub delta: f64,
    /// Positive delta means regression (worse), negative means improvement.
    pub is_regression: bool,
}

/// Compare a current run against a baseline, identifying regressions and improvements.
pub fn compare_runs(baseline: &RunResult, current: &RunResult, threshold: f64) -> Vec<MetricDelta> {
    let mut deltas = Vec::new();

    for current_scenario in &current.scenarios {
        let Some(baseline_scenario) = baseline
            .scenarios
            .iter()
            .find(|b| b.scenario_id == current_scenario.scenario_id)
        else {
            continue;
        };

        let id = current_scenario.scenario_id.as_str();
        let b = &baseline_scenario.trace;
        let c = &current_scenario.trace;

        // Wall time comparison.
        record_delta(
            id,
            "wall_time_ms",
            b.wall_time_ms as f64,
            c.wall_time_ms as f64,
            threshold,
            &mut deltas,
        );

        // Token count comparison (input + output).
        record_delta(
            id,
            "total_tokens",
            (b.input_tokens + b.output_tokens) as f64,
            (c.input_tokens + c.output_tokens) as f64,
            threshold,
            &mut deltas,
        );

        // LLM calls comparison.
        record_delta(
            id,
            "llm_calls",
            b.llm_calls as f64,
            c.llm_calls as f64,
            threshold,
            &mut deltas,
        );

        // Tool call count comparison.
        record_delta(
            id,
            "tool_calls",
            b.tool_calls.len() as f64,
            c.tool_calls.len() as f64,
            threshold,
            &mut deltas,
        );
    }

    deltas
}

/// Record one metric's delta when it moved by more than `threshold`.
///
/// A zero baseline has no meaningful relative delta, so it is skipped.
fn record_delta(
    scenario_id: &str,
    metric: &str,
    baseline: f64,
    current: f64,
    threshold: f64,
    deltas: &mut Vec<MetricDelta>,
) {
    if baseline <= 0.0 {
        return;
    }
    let delta = (current - baseline) / baseline;
    if delta.abs() > threshold {
        deltas.push(MetricDelta {
            scenario_id: scenario_id.to_string(),
            metric: metric.to_string(),
            baseline,
            current,
            delta,
            is_regression: delta > 0.0,
        });
    }
}
