//! Cost enforcement guardrails for the agent.
//!
//! Tracks LLM spending and action rates, enforcing configurable limits
//! to prevent runaway agents from burning through API credits. Especially
//! important for daemon/heartbeat modes where the agent acts autonomously.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use tokio::sync::Mutex;

use crate::llm::costs;

/// Configuration for cost guardrails.
#[derive(Debug, Clone, Default)]
pub struct CostGuardConfig {
    /// Maximum spend per day in cents (e.g. 10000 = $100). None = unlimited.
    pub max_cost_per_day_cents: Option<u64>,
    /// Maximum LLM calls per hour. None = unlimited.
    pub max_actions_per_hour: Option<u64>,
}

/// Error returned when a cost limit is exceeded.
#[derive(Debug, Clone)]
pub enum CostLimitExceeded {
    /// Daily spending cap reached.
    DailyBudget { spent_cents: u64, limit_cents: u64 },
    /// Hourly action rate limit reached.
    HourlyRate { actions: u64, limit: u64 },
}

impl std::fmt::Display for CostLimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DailyBudget {
                spent_cents,
                limit_cents,
            } => write!(
                f,
                "Daily cost limit exceeded: spent ${:.2} of ${:.2} allowed",
                *spent_cents as f64 / 100.0,
                *limit_cents as f64 / 100.0
            ),
            Self::HourlyRate { actions, limit } => write!(
                f,
                "Hourly action limit exceeded: {} actions of {} allowed per hour",
                actions, limit
            ),
        }
    }
}

/// Per-model token usage counters.
#[derive(Debug, Clone, Default)]
pub struct ModelTokens {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost: Decimal,
}

/// Tracks costs and action rates, enforcing configurable limits.
///
/// Thread-safe; designed to be shared via `Arc<CostGuard>`.
pub struct CostGuard {
    config: CostGuardConfig,

    /// Running cost total for the current day (in USD, not cents).
    daily_cost: Mutex<DailyCost>,

    /// Sliding window of action timestamps for rate limiting.
    action_window: Mutex<VecDeque<Instant>>,

    /// Flag set when daily budget is exceeded to short-circuit checks.
    budget_exceeded: AtomicBool,

    /// Per-model token usage since startup.
    model_tokens: Mutex<HashMap<String, ModelTokens>>,
}

struct DailyCost {
    total: Decimal,
    /// Day boundary (midnight UTC) for resetting the counter.
    reset_date: chrono::NaiveDate,
}

impl CostGuard {
    pub fn new(config: CostGuardConfig) -> Self {
        Self {
            config,
            daily_cost: Mutex::new(DailyCost {
                total: Decimal::ZERO,
                reset_date: chrono::Utc::now().date_naive(),
            }),
            action_window: Mutex::new(VecDeque::new()),
            budget_exceeded: AtomicBool::new(false),
            model_tokens: Mutex::new(HashMap::new()),
        }
    }

    /// Check whether the next action is allowed under the configured limits.
    ///
    /// Call this BEFORE making an LLM call. Does NOT record the action yet,
    /// call `record_action` after the action completes.
    pub async fn check_allowed(&self) -> Result<(), CostLimitExceeded> {
        // Fast path: if budget already blown, skip the lock
        if self.budget_exceeded.load(Ordering::Relaxed) {
            let daily = self.daily_cost.lock().await;
            let spent_cents = to_cents(daily.total);
            return Err(CostLimitExceeded::DailyBudget {
                spent_cents,
                limit_cents: self.config.max_cost_per_day_cents.unwrap_or(0),
            });
        }

        // Check daily budget
        if let Some(limit_cents) = self.config.max_cost_per_day_cents {
            let daily = self.daily_cost.lock().await;
            let spent_cents = to_cents(daily.total);
            if spent_cents >= limit_cents {
                self.budget_exceeded.store(true, Ordering::Relaxed);
                return Err(CostLimitExceeded::DailyBudget {
                    spent_cents,
                    limit_cents,
                });
            }
        }

        // Check hourly rate
        if let Some(limit) = self.config.max_actions_per_hour {
            let mut window = self.action_window.lock().await;
            // checked_sub avoids panic when system uptime < 1 hour (Windows)
            if let Some(cutoff) = Instant::now().checked_sub(std::time::Duration::from_secs(3600)) {
                // Drain expired entries
                while window.front().is_some_and(|t| *t < cutoff) {
                    window.pop_front();
                }
            }
            let count = window.len() as u64;
            if count >= limit {
                return Err(CostLimitExceeded::HourlyRate {
                    actions: count,
                    limit,
                });
            }
        }

        Ok(())
    }

    /// Record a completed LLM action: its token costs and the action timestamp.
    ///
    /// Call this AFTER an LLM call completes so that costs are tracked.
    /// - `cache_read_input_tokens`: tokens served from cache.
    /// - `cache_creation_input_tokens`: tokens written to cache.
    /// - `cache_read_discount`: divisor for cache-read cost (e.g. 10 for Anthropic 90% off, 2 for OpenAI 50% off).
    /// - `cache_write_multiplier`: cost multiplier for cache writes (1.25 for 5m, 2.0 for 1h).
    ///
    /// When `cost_per_token` is `Some`, those rates are used directly (provider-
    /// sourced pricing). When `None`, falls back to the static `costs::model_cost`
    /// lookup table, then `costs::default_cost`.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_llm_call(
        &self,
        model: &str,
        input_tokens: u32,
        output_tokens: u32,
        cache_read_input_tokens: u32,
        cache_creation_input_tokens: u32,
        cache_read_discount: Decimal,
        cache_write_multiplier: Decimal,
        cost_per_token: Option<(Decimal, Decimal)>,
    ) -> Decimal {
        let (input_rate, output_rate) = cost_per_token
            .unwrap_or_else(|| costs::model_cost(model).unwrap_or_else(costs::default_cost));
        // Cached read tokens cost input_rate / cache_read_discount (provider-specific).
        // Cached write tokens cost write_multiplier × input_rate (e.g. 1.25× for 5m, 2× for 1h).
        // Uncached tokens = total input - cache reads - cache writes.
        let cached_total = cache_read_input_tokens.saturating_add(cache_creation_input_tokens);
        let uncached_input = input_tokens.saturating_sub(cached_total);
        let effective_discount = if cache_read_discount.is_zero() {
            Decimal::ONE
        } else {
            cache_read_discount
        };
        let cache_read_cost =
            input_rate * Decimal::from(cache_read_input_tokens) / effective_discount;
        let cache_write_cost =
            input_rate * Decimal::from(cache_creation_input_tokens) * cache_write_multiplier;
        let cost = input_rate * Decimal::from(uncached_input)
            + cache_read_cost
            + cache_write_cost
            + output_rate * Decimal::from(output_tokens);

        // Update daily cost (reset if new day)
        {
            let mut daily = self.daily_cost.lock().await;
            let today = chrono::Utc::now().date_naive();
            if today != daily.reset_date {
                daily.total = Decimal::ZERO;
                daily.reset_date = today;
                self.budget_exceeded.store(false, Ordering::Relaxed);
                tracing::info!("Cost guard: daily counter reset for {}", today);
            }
            daily.total += cost;

            // Check if we just crossed the threshold
            if let Some(limit_cents) = self.config.max_cost_per_day_cents {
                let spent_cents = to_cents(daily.total);
                if spent_cents >= limit_cents {
                    self.budget_exceeded.store(true, Ordering::Relaxed);
                    tracing::warn!(
                        "Daily cost limit reached: ${:.2} of ${:.2}",
                        daily.total,
                        Decimal::from(limit_cents) / dec!(100)
                    );
                }
                // Warn at 80% threshold
                let warn_threshold = limit_cents * 80 / 100;
                if spent_cents >= warn_threshold && spent_cents < limit_cents {
                    tracing::warn!(
                        "Approaching daily cost limit: ${:.2} of ${:.2} ({}%)",
                        daily.total,
                        Decimal::from(limit_cents) / dec!(100),
                        spent_cents * 100 / limit_cents
                    );
                }
            }
        }

        // Record action in sliding window
        {
            let mut window = self.action_window.lock().await;
            window.push_back(Instant::now());
        }

        // Track per-model token usage
        {
            let mut tokens = self.model_tokens.lock().await;
            let entry = tokens.entry(model.to_string()).or_default();
            entry.input_tokens += u64::from(input_tokens);
            entry.output_tokens += u64::from(output_tokens);
            entry.cost += cost;
        }

        cost
    }

    /// Current daily spend in USD (as Decimal).
    pub async fn daily_spend(&self) -> Decimal {
        let daily = self.daily_cost.lock().await;
        let today = chrono::Utc::now().date_naive();
        if today != daily.reset_date {
            Decimal::ZERO
        } else {
            daily.total
        }
    }

    /// Number of actions in the current hourly window.
    pub async fn actions_this_hour(&self) -> u64 {
        let mut window = self.action_window.lock().await;
        // checked_sub avoids panic when system uptime < 1 hour (Windows)
        if let Some(cutoff) = Instant::now().checked_sub(std::time::Duration::from_secs(3600)) {
            while window.front().is_some_and(|t| *t < cutoff) {
                window.pop_front();
            }
        }
        window.len() as u64
    }

    /// Per-model token usage since startup.
    pub async fn model_usage(&self) -> HashMap<String, ModelTokens> {
        self.model_tokens.lock().await.clone()
    }
}

/// Convert a Decimal USD amount to whole cents (truncated).
fn to_cents(usd: Decimal) -> u64 {
    let cents = (usd * dec!(100)).trunc();
    cents.to_string().parse::<u64>().unwrap_or(0)
}

#[cfg(test)]
mod tests;
