//! Tests for the delayed restart's injectable exit seam.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

/// Records how often the delayed restart task asked to exit.
#[derive(Default)]
struct RecordingExit {
    calls: AtomicUsize,
}

impl RestartExit for RecordingExit {
    fn exit(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

/// Yield to the scheduler until the spawned restart task has run.
async fn settle() {
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
}

/// `RestartTool::execute` must defer to its snapshot after the delay.
///
/// The recording exit seam stands in for `std::process::exit(0)`, so this
/// exercises the spawned task itself: a disabling snapshot value must not
/// request the exit, and an absent or non-disabling value must request it
/// once, after the delay has elapsed.
#[tokio::test(start_paused = true)]
async fn restart_execute_exits_only_when_the_snapshot_allows_it() {
    let ctx = crate::context::JobContext::new("test", "test restart");
    let cases: [(&str, Option<&str>, usize); 4] = [
        ("absent", None, 1),
        ("1", Some("1"), 0),
        ("true", Some("true"), 0),
        ("false", Some("false"), 1),
    ];

    for (label, disable_restart, expected) in cases {
        let recorder = Arc::new(RecordingExit::default());
        let mut env = EnvContext::default().with_env("AXINITE_IN_DOCKER", "true");
        if let Some(value) = disable_restart {
            env = env.with_env("AXINITE_DISABLE_RESTART", value);
        }
        let tool = RestartTool::with_exit(env, recorder.clone());

        tool.execute(serde_json::json!({"delay_secs": 1}), &ctx)
            .await
            .expect("a docker restart should be accepted");

        // The exit is deferred until the delay elapses.
        settle().await;
        assert_eq!(
            recorder.calls.load(Ordering::SeqCst),
            0,
            "case {label}: exit requested before the delay elapsed"
        );

        tokio::time::advance(Duration::from_secs(2)).await;
        settle().await;
        assert_eq!(
            recorder.calls.load(Ordering::SeqCst),
            expected,
            "case {label}: exit requests after the delay"
        );
    }
}
