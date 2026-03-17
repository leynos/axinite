//! Constants and helpers for locating test fixture files.

use std::time::Duration;

/// Fixture root under `$CARGO_MANIFEST_DIR/tests/fixtures/llm_traces/`.
#[allow(dead_code)]
pub const FIXTURE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/llm_traces");

/// Default timeout for single-turn trace tests (15 s).
#[allow(dead_code)]
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// Longer timeout for multi-turn or routine-heavy tests (30 s).
#[allow(dead_code)]
pub const LONG_TIMEOUT: Duration = Duration::from_secs(30);

/// Build a full fixture path from a subdirectory and filename.
///
/// ```ignore
/// let path = fixture_path("spot", "smoke_greeting.json");
/// // => "<manifest>/tests/fixtures/llm_traces/spot/smoke_greeting.json"
/// ```
#[allow(dead_code)]
pub fn fixture_path(subdir: &str, filename: &str) -> String {
    format!("{FIXTURE_ROOT}/{subdir}/{filename}")
}
