//! Precision tests for self-repair duration helpers.

use std::time::Duration;

use chrono::Utc;

use crate::agent::self_repair::default::duration_since;

#[test]
fn duration_since_millisecond_precision() {
    use chrono::Duration as ChronoDuration;

    let now = Utc::now();
    let start = now - ChronoDuration::milliseconds(500);
    let elapsed = duration_since(now, start);

    // Should be >= 500ms and < 1s (proving millisecond resolution, not second)
    assert!(
        elapsed >= Duration::from_millis(500),
        "Expected >= 500ms, got {:?}",
        elapsed
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "Expected < 1s, got {:?}",
        elapsed
    );
}
