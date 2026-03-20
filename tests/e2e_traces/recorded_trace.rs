//! E2E tests for recorded LLM traces.
//!
//! Each test replays a recorded fixture through the full agent loop, verifying
//! declarative `expects` from the JSON and any additional manual checks.

use crate::support::test_rig::run_recorded_trace;
use rstest::rstest;

/// Recorded trace tests covering telegram check, weather query, and baseball stats.
#[rstest]
#[case("telegram_check.json")]
#[case("weather_sf.json")]
#[case("baseball_stats.json")]
#[tokio::test]
async fn recorded_trace(#[case] fixture_name: &str) {
    run_recorded_trace(fixture_name).await;
}
