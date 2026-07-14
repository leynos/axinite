//! Smart routing provider that classifies request complexity and routes to
//! the cheap or primary model, with optional cascade escalation.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use regex::Regex;
use rust_decimal::Decimal;

use crate::llm::error::LlmError;
use crate::llm::provider::{
    CompletionRequest, CompletionResponse, LlmProvider, ModelMetadata, Role, ToolCompletionRequest,
    ToolCompletionResponse,
};

use super::patterns::{DEFAULT_OVERRIDES, RE_DOMAIN_DEFAULT, RE_TIER_HINT, build_domain_regex};
use super::scorer::{ScorerConfig, ScorerWeights, score_complexity_with_regex};
use super::tiers::{TaskComplexity, Tier};

/// Configuration for the smart routing provider.
#[derive(Debug, Clone)]
pub struct SmartRoutingConfig {
    /// Enable cascade mode: retry with primary if cheap model response seems uncertain.
    pub cascade_enabled: bool,
    /// Custom domain keywords for the scorer (None uses defaults).
    pub domain_keywords: Option<Vec<String>>,
}

impl Default for SmartRoutingConfig {
    fn default() -> Self {
        Self {
            cascade_enabled: true,
            domain_keywords: None,
        }
    }
}

/// Atomic counters for routing observability.
struct SmartRoutingStats {
    total_requests: AtomicU64,
    cheap_requests: AtomicU64,
    primary_requests: AtomicU64,
    cascade_escalations: AtomicU64,
}

impl SmartRoutingStats {
    fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            cheap_requests: AtomicU64::new(0),
            primary_requests: AtomicU64::new(0),
            cascade_escalations: AtomicU64::new(0),
        }
    }
}

/// Snapshot of routing statistics for external consumption.
#[derive(Debug, Clone)]
pub struct SmartRoutingSnapshot {
    pub total_requests: u64,
    pub cheap_requests: u64,
    pub primary_requests: u64,
    pub cascade_escalations: u64,
}

/// Smart routing provider that classifies task complexity and routes to the appropriate model.
///
/// - `complete()` — scores complexity across 13 dimensions, checks pattern overrides, then
///   routes to cheap or primary model. Moderate tasks use cascade (try cheap, escalate if uncertain).
/// - `complete_with_tools()` — always routes to primary (tool use requires reliable structured output)
pub struct SmartRoutingProvider {
    primary: Arc<dyn LlmProvider>,
    cheap: Arc<dyn LlmProvider>,
    config: SmartRoutingConfig,
    scorer_config: ScorerConfig,
    /// Pre-compiled domain regex (built once at construction time).
    domain_regex: Regex,
    stats: SmartRoutingStats,
}

impl SmartRoutingProvider {
    /// Create a new smart routing provider wrapping a primary and cheap provider.
    pub fn new(
        primary: Arc<dyn LlmProvider>,
        cheap: Arc<dyn LlmProvider>,
        config: SmartRoutingConfig,
    ) -> Self {
        let scorer_config = ScorerConfig {
            weights: ScorerWeights::default(),
            domain_keywords: config.domain_keywords.clone(),
        };
        let domain_regex = match &scorer_config.domain_keywords {
            Some(custom) => {
                let refs: Vec<&str> = custom.iter().map(|s| s.as_str()).collect();
                build_domain_regex(&refs)
            }
            None => RE_DOMAIN_DEFAULT.clone(),
        };
        Self {
            primary,
            cheap,
            config,
            scorer_config,
            domain_regex,
            stats: SmartRoutingStats::new(),
        }
    }

    /// Get a snapshot of routing statistics.
    pub fn stats(&self) -> SmartRoutingSnapshot {
        SmartRoutingSnapshot {
            total_requests: self.stats.total_requests.load(Ordering::Relaxed),
            cheap_requests: self.stats.cheap_requests.load(Ordering::Relaxed),
            primary_requests: self.stats.primary_requests.load(Ordering::Relaxed),
            cascade_escalations: self.stats.cascade_escalations.load(Ordering::Relaxed),
        }
    }

    /// Classify the complexity of a request based on its last user message.
    ///
    /// Priority: explicit tier hints > pattern overrides > 13-dimension scorer.
    pub(super) fn classify(&self, request: &CompletionRequest) -> TaskComplexity {
        let last_user_msg = last_user_message(request);

        if let Some(complexity) = tier_hint_complexity(last_user_msg) {
            return complexity;
        }

        if let Some(complexity) = pattern_override_complexity(last_user_msg) {
            return complexity;
        }

        self.scored_complexity(last_user_msg)
    }

    /// Classify via the full 13-dimension scorer (uses the pre-compiled
    /// domain regex built at construction time).
    fn scored_complexity(&self, last_user_msg: &str) -> TaskComplexity {
        let breakdown = score_complexity_with_regex(
            last_user_msg,
            &self.scorer_config.weights,
            &self.domain_regex,
        );
        let complexity = TaskComplexity::from(breakdown.tier);
        tracing::trace!(
            score = breakdown.total,
            tier = %breakdown.tier,
            ?complexity,
            hints = ?breakdown.hints,
            "Smart routing: scored complexity"
        );
        complexity
    }

    /// Check if a response from the cheap model shows uncertainty, warranting escalation.
    pub(super) fn response_is_uncertain(response: &CompletionResponse) -> bool {
        let content = response.content.trim();

        // Empty response is always uncertain
        if content.is_empty() {
            return true;
        }

        let lower = content.to_lowercase();

        // Uncertainty signals
        let uncertainty_patterns = [
            "i'm not sure",
            "i am not sure",
            "i don't know",
            "i do not know",
            "i'm unable to",
            "i am unable to",
            "i cannot",
            "i can't",
            "beyond my capabilities",
            "beyond my ability",
            "i'm not able to",
            "i am not able to",
            "i don't have enough",
            "i do not have enough",
            "i need more context",
            "i need more information",
            "could you clarify",
            "could you provide more",
            "i'm not confident",
            "i am not confident",
        ];

        uncertainty_patterns.iter().any(|p| lower.contains(p))
    }
}

/// Extract the trimmed content of the last user message in a request.
///
/// Trimming keeps anchored regexes and token scoring consistent regardless
/// of leading or trailing whitespace.
fn last_user_message(request: &CompletionRequest) -> &str {
    request
        .messages
        .iter()
        .rev()
        .find(|m| m.role == Role::User)
        .map(|m| m.content.as_str())
        .unwrap_or("")
        .trim()
}

/// Classify from an explicit tier hint (e.g. "[tier:flash]"), if present.
///
/// Explicit hints take highest priority over pattern overrides and scoring.
fn tier_hint_complexity(last_user_msg: &str) -> Option<TaskComplexity> {
    let caps = RE_TIER_HINT.captures(last_user_msg)?;
    // Group 1 always exists when the regex matches; an empty string
    // falls through to the defensive branch below.
    let tier_str = caps.get(1).map_or("", |m| m.as_str());
    let tier = match tier_str.to_lowercase().as_str() {
        "flash" => Tier::Flash,
        "standard" => Tier::Standard,
        "pro" => Tier::Pro,
        "frontier" => Tier::Frontier,
        other => {
            tracing::error!(tier = %other, "Unexpected tier in hint despite regex constraint");
            Tier::Standard
        }
    };
    let complexity = TaskComplexity::from(tier);
    tracing::trace!(
        %tier,
        ?complexity,
        "Smart routing: explicit tier hint"
    );
    Some(complexity)
}

/// Classify from the fast-path pattern overrides, if any pattern matches.
fn pattern_override_complexity(last_user_msg: &str) -> Option<TaskComplexity> {
    for po in DEFAULT_OVERRIDES.iter() {
        if po.regex.is_match(last_user_msg) {
            let complexity = TaskComplexity::from(po.tier);
            tracing::trace!(
                tier = %po.tier,
                ?complexity,
                "Smart routing: pattern override matched"
            );
            return Some(complexity);
        }
    }
    None
}

impl crate::llm::NativeLlmProvider for SmartRoutingProvider {
    fn model_name(&self) -> &str {
        self.primary.model_name()
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        self.primary.cost_per_token()
    }

    fn cache_write_multiplier(&self) -> Decimal {
        self.primary.cache_write_multiplier()
    }

    fn cache_read_discount(&self) -> Decimal {
        self.primary.cache_read_discount()
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.stats.total_requests.fetch_add(1, Ordering::Relaxed);

        let complexity = self.classify(&request);

        match complexity {
            TaskComplexity::Simple => {
                tracing::trace!(
                    model = %self.cheap.model_name(),
                    "Smart routing: Simple task -> cheap model"
                );
                self.stats.cheap_requests.fetch_add(1, Ordering::Relaxed);
                self.cheap.complete(request).await
            }
            TaskComplexity::Complex => {
                tracing::trace!(
                    model = %self.primary.model_name(),
                    "Smart routing: Complex task -> primary model"
                );
                self.stats.primary_requests.fetch_add(1, Ordering::Relaxed);
                self.primary.complete(request).await
            }
            TaskComplexity::Moderate => {
                if self.config.cascade_enabled {
                    tracing::trace!(
                        model = %self.cheap.model_name(),
                        "Smart routing: Moderate task -> cheap model (cascade enabled)"
                    );
                    self.stats.cheap_requests.fetch_add(1, Ordering::Relaxed);

                    let response = self.cheap.complete(request.clone()).await?;

                    if Self::response_is_uncertain(&response) {
                        tracing::info!(
                            cheap_model = %self.cheap.model_name(),
                            primary_model = %self.primary.model_name(),
                            "Smart routing: Escalating to primary (cheap model response uncertain)"
                        );
                        self.stats
                            .cascade_escalations
                            .fetch_add(1, Ordering::Relaxed);
                        self.stats.primary_requests.fetch_add(1, Ordering::Relaxed);
                        self.primary.complete(request).await
                    } else {
                        Ok(response)
                    }
                } else {
                    // Without cascade, moderate tasks go to cheap model
                    tracing::trace!(
                        model = %self.cheap.model_name(),
                        "Smart routing: Moderate task -> cheap model (cascade disabled)"
                    );
                    self.stats.cheap_requests.fetch_add(1, Ordering::Relaxed);
                    self.cheap.complete(request).await
                }
            }
        }
    }

    /// Tool use always goes to the primary model for reliable structured output.
    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        self.stats.total_requests.fetch_add(1, Ordering::Relaxed);
        self.stats.primary_requests.fetch_add(1, Ordering::Relaxed);
        tracing::trace!(
            model = %self.primary.model_name(),
            "Smart routing: Tool use -> primary model (always)"
        );
        self.primary.complete_with_tools(request).await
    }

    async fn list_models(&self) -> Result<Vec<String>, LlmError> {
        self.primary.list_models().await
    }

    async fn model_metadata(&self) -> Result<ModelMetadata, LlmError> {
        self.primary.model_metadata().await
    }

    fn effective_model_name(&self, requested_model: Option<&str>) -> String {
        self.primary.effective_model_name(requested_model)
    }

    fn active_model_name(&self) -> String {
        self.primary.active_model_name()
    }

    fn set_model(&self, model: &str) -> Result<(), LlmError> {
        self.primary.set_model(model)
    }

    fn calculate_cost(&self, input_tokens: u32, output_tokens: u32) -> Decimal {
        self.primary.calculate_cost(input_tokens, output_tokens)
    }
}
