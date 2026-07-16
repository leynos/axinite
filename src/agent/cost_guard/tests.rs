//! Unit tests for cost guard budget and rate-limit enforcement.

use super::*;

#[tokio::test]
async fn test_unlimited_allows_everything() {
    let guard = CostGuard::new(CostGuardConfig::default());

    // No limits set, should always be allowed
    assert!(guard.check_allowed().await.is_ok());

    // Record a big call, still allowed
    guard
        .record_llm_call(
            "gpt-4o",
            100_000,
            100_000,
            0,
            0,
            Decimal::ONE,
            Decimal::ONE,
            None,
        )
        .await;
    assert!(guard.check_allowed().await.is_ok());
}

#[tokio::test]
async fn test_daily_budget_enforcement() {
    let guard = CostGuard::new(CostGuardConfig {
        max_cost_per_day_cents: Some(1), // $0.01 limit
        max_actions_per_hour: None,
    });

    // First call allowed
    assert!(guard.check_allowed().await.is_ok());

    // Record a call that costs more than $0.01
    // gpt-4o: input=$0.0000025/tok, output=$0.00001/tok
    // 10000 input + 10000 output = $0.025 + $0.10 = $0.125
    guard
        .record_llm_call(
            "gpt-4o",
            10_000,
            10_000,
            0,
            0,
            Decimal::ONE,
            Decimal::ONE,
            None,
        )
        .await;

    // Now should be blocked
    let result = guard.check_allowed().await;
    assert!(result.is_err());
    match result.unwrap_err() {
        CostLimitExceeded::DailyBudget { limit_cents, .. } => {
            assert_eq!(limit_cents, 1);
        }
        other => panic!("Expected DailyBudget, got {:?}", other),
    }
}

#[tokio::test]
async fn test_hourly_rate_enforcement() {
    let guard = CostGuard::new(CostGuardConfig {
        max_cost_per_day_cents: None,
        max_actions_per_hour: Some(3),
    });

    // First 3 actions allowed
    for _ in 0..3 {
        assert!(guard.check_allowed().await.is_ok());
        guard
            .record_llm_call("gpt-4o", 10, 10, 0, 0, Decimal::ONE, Decimal::ONE, None)
            .await;
    }

    // 4th should be blocked
    let result = guard.check_allowed().await;
    assert!(result.is_err());
    match result.unwrap_err() {
        CostLimitExceeded::HourlyRate { actions, limit } => {
            assert_eq!(actions, 3);
            assert_eq!(limit, 3);
        }
        other => panic!("Expected HourlyRate, got {:?}", other),
    }
}

#[tokio::test]
async fn test_daily_spend_tracking() {
    let guard = CostGuard::new(CostGuardConfig::default());

    assert_eq!(guard.daily_spend().await, Decimal::ZERO);

    let cost = guard
        .record_llm_call("gpt-4o", 1000, 500, 0, 0, Decimal::ONE, Decimal::ONE, None)
        .await;
    assert!(cost > Decimal::ZERO);
    assert_eq!(guard.daily_spend().await, cost);
}

#[tokio::test]
async fn test_actions_this_hour() {
    let guard = CostGuard::new(CostGuardConfig::default());

    assert_eq!(guard.actions_this_hour().await, 0);

    guard
        .record_llm_call("gpt-4o", 10, 10, 0, 0, Decimal::ONE, Decimal::ONE, None)
        .await;
    guard
        .record_llm_call("gpt-4o", 10, 10, 0, 0, Decimal::ONE, Decimal::ONE, None)
        .await;

    assert_eq!(guard.actions_this_hour().await, 2);
}

#[test]
fn test_to_cents() {
    assert_eq!(to_cents(dec!(1.50)), 150);
    assert_eq!(to_cents(dec!(0.01)), 1);
    assert_eq!(to_cents(Decimal::ZERO), 0);
}

#[test]
fn test_cost_limit_display() {
    let budget = CostLimitExceeded::DailyBudget {
        spent_cents: 1050,
        limit_cents: 1000,
    };
    assert!(budget.to_string().contains("$10.50"));
    assert!(budget.to_string().contains("$10.00"));

    let rate = CostLimitExceeded::HourlyRate {
        actions: 101,
        limit: 100,
    };
    assert!(rate.to_string().contains("101 actions"));
    assert!(rate.to_string().contains("100 allowed"));
}

#[tokio::test]
async fn test_model_usage_per_model_tracking() {
    let guard = CostGuard::new(CostGuardConfig::default());

    // Initially empty
    assert!(guard.model_usage().await.is_empty());

    // Record calls for two different models
    guard
        .record_llm_call("gpt-4o", 1000, 500, 0, 0, Decimal::ONE, Decimal::ONE, None)
        .await;
    guard
        .record_llm_call("gpt-4o", 2000, 1000, 0, 0, Decimal::ONE, Decimal::ONE, None)
        .await;
    guard
        .record_llm_call(
            "claude-3-5-sonnet-20241022",
            500,
            200,
            0,
            0,
            Decimal::ONE,
            Decimal::ONE,
            None,
        )
        .await;

    let usage = guard.model_usage().await;
    assert_eq!(usage.len(), 2);

    let gpt = usage.get("gpt-4o").expect("gpt-4o should be tracked");
    assert_eq!(gpt.input_tokens, 3000);
    assert_eq!(gpt.output_tokens, 1500);
    assert!(gpt.cost > Decimal::ZERO);

    let claude = usage
        .get("claude-3-5-sonnet-20241022")
        .expect("claude should be tracked");
    assert_eq!(claude.input_tokens, 500);
    assert_eq!(claude.output_tokens, 200);
    assert!(claude.cost > Decimal::ZERO);

    // Costs should differ since models have different pricing
    assert_ne!(gpt.cost, claude.cost);
}

#[tokio::test]
async fn test_cache_discount_reduces_cost() {
    let guard = CostGuard::new(CostGuardConfig::default());

    // Full price: 1000 input + 500 output, no cache
    let full_cost = guard
        .record_llm_call(
            "claude-opus-4-6",
            1000,
            500,
            0,
            0,
            Decimal::ONE,
            Decimal::ONE,
            None,
        )
        .await;

    let guard2 = CostGuard::new(CostGuardConfig::default());

    // Same tokens but all input cached (90% discount on input)
    let cached_cost = guard2
        .record_llm_call(
            "claude-opus-4-6",
            1000,
            500,
            1000,
            0,
            dec!(10),
            Decimal::ONE,
            None,
        )
        .await;

    // Cached cost must be strictly less than full cost
    assert!(
        cached_cost < full_cost,
        "cached_cost ({}) should be less than full_cost ({})",
        cached_cost,
        full_cost
    );

    // The difference should be exactly 90% of the input cost
    let (input_rate, _) = costs::model_cost("claude-opus-4-6").unwrap();
    let expected_savings = input_rate * Decimal::from(1000u32) * dec!(9) / dec!(10);
    let actual_savings = full_cost - cached_cost;
    assert_eq!(
        actual_savings, expected_savings,
        "savings should be 90% of input cost for fully-cached request"
    );
}

#[tokio::test]
async fn test_cache_write_surcharge_increases_cost() {
    let guard = CostGuard::new(CostGuardConfig::default());

    // Full price: 1000 input + 500 output, no cache activity
    let full_cost = guard
        .record_llm_call(
            "claude-opus-4-6",
            1000,
            500,
            0,
            0,
            Decimal::ONE,
            Decimal::ONE,
            None,
        )
        .await;

    let guard2 = CostGuard::new(CostGuardConfig::default());

    // Same tokens, but all input tokens are cache writes (1.25x surcharge for 5m TTL)
    let short_multiplier = Decimal::new(125, 2); // 1.25
    let write_cost = guard2
        .record_llm_call(
            "claude-opus-4-6",
            1000,
            500,
            0,
            1000,
            Decimal::ONE,
            short_multiplier,
            None,
        )
        .await;

    // Write cost must be strictly greater than full cost
    assert!(
        write_cost > full_cost,
        "write_cost ({}) should be greater than full_cost ({})",
        write_cost,
        full_cost
    );

    // The difference should be exactly 25% of the input cost
    let (input_rate, _) = costs::model_cost("claude-opus-4-6").unwrap();
    let expected_surcharge = input_rate * Decimal::from(1000u32) * dec!(0.25);
    let actual_surcharge = write_cost - full_cost;
    assert_eq!(
        actual_surcharge, expected_surcharge,
        "surcharge should be 25% of input cost for 5m cache writes"
    );
}

#[tokio::test]
async fn test_cache_write_surcharge_long_ttl() {
    let guard = CostGuard::new(CostGuardConfig::default());

    // Full price: 1000 input + 500 output
    let full_cost = guard
        .record_llm_call(
            "claude-opus-4-6",
            1000,
            500,
            0,
            0,
            Decimal::ONE,
            Decimal::ONE,
            None,
        )
        .await;

    let guard2 = CostGuard::new(CostGuardConfig::default());

    // All input tokens are cache writes with 2.0x multiplier (1h TTL)
    let long_multiplier = Decimal::TWO;
    let write_cost = guard2
        .record_llm_call(
            "claude-opus-4-6",
            1000,
            500,
            0,
            1000,
            Decimal::ONE,
            long_multiplier,
            None,
        )
        .await;

    // Write cost > full cost
    assert!(write_cost > full_cost);

    // Surcharge should be 100% of input cost (2.0x - 1.0x = 1.0x)
    let (input_rate, _) = costs::model_cost("claude-opus-4-6").unwrap();
    let expected_surcharge = input_rate * Decimal::from(1000u32);
    let actual_surcharge = write_cost - full_cost;
    assert_eq!(
        actual_surcharge, expected_surcharge,
        "surcharge should be 100% of input cost for 1h cache writes"
    );
}

/// Regression test for #657: Instant::now() - Duration panics on Windows
/// when system uptime is less than the subtracted duration.
#[tokio::test]
async fn test_checked_sub_no_panic_on_fresh_guard() {
    // A fresh CostGuard with rate limits should not panic even if
    // checked_sub returns None (simulating short uptime).
    let guard = CostGuard::new(CostGuardConfig {
        max_cost_per_day_cents: None,
        max_actions_per_hour: Some(100),
    });

    // These must not panic regardless of system uptime
    assert!(guard.check_allowed().await.is_ok());
    assert_eq!(guard.actions_this_hour().await, 0);

    // Record some actions and verify again
    guard
        .record_llm_call("gpt-4o", 10, 10, 0, 0, Decimal::ONE, Decimal::ONE, None)
        .await;
    assert!(guard.check_allowed().await.is_ok());
    assert_eq!(guard.actions_this_hour().await, 1);
}

/// Verify that checked_sub itself behaves as expected for the pattern we use.
#[test]
fn test_instant_checked_sub_returns_none_for_overflow() {
    // Duration::MAX will always exceed uptime, so checked_sub must return None
    let result = Instant::now().checked_sub(std::time::Duration::MAX);
    assert!(result.is_none());
}
