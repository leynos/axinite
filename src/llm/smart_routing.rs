//! Smart routing provider that routes requests to cheap or primary models based on task complexity.
//!
//! Uses a 13-dimension complexity scorer (from PR #208 by @onlyamicrowave) to analyze prompts
//! across reasoning, code, multi-step, domain-specific, creativity, precision, safety, and other
//! dimensions. Pattern overrides provide fast-path routing for obvious cases (greetings → cheap,
//! security audits → primary).
//!
//! This is a decorator that wraps two `LlmProvider`s and implements `LlmProvider` itself,
//! following the same pattern as `RetryProvider`, `CachedProvider`, and `CircuitBreakerProvider`.
//!
//! # Complexity Tiers
//!
//! The scorer produces a 0-100 score mapped to four tiers:
//! - **Flash** (0-15): Greetings, quick lookups → cheap model
//! - **Standard** (16-40): Writing, comparisons → cheap model
//! - **Pro** (41-65): Multi-step analysis, code review → cheap with cascade, or primary
//! - **Frontier** (66+): Security audits, critical decisions → primary model

mod keywords;
mod patterns;
mod provider;
mod scorer;
mod tiers;

#[cfg(test)]
mod tests;

pub use keywords::DEFAULT_DOMAIN_KEYWORDS;
pub use provider::{SmartRoutingConfig, SmartRoutingProvider, SmartRoutingSnapshot};
pub use scorer::{
    ScoreBreakdown, ScorerConfig, ScorerWeights, score_complexity, score_complexity_with_config,
    score_complexity_with_regex,
};
pub use tiers::{TaskComplexity, Tier};
