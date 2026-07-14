//! Shared assertion helpers for build result tests.

use super::super::{BuildLog, BuildPhase, BuildResult};

pub(super) fn assert_build_success(res: &BuildResult) {
    assert!(res.success, "expected build to succeed");
    assert!(
        res.error.is_none(),
        "expected successful build to have no error, got {:?}",
        res.error
    );
}

pub(super) fn assert_build_failure_contains(res: &BuildResult, needle: &str) {
    assert!(!res.success, "expected build to fail");
    assert!(
        res.error
            .as_deref()
            .is_some_and(|error| error.contains(needle)),
        "expected build error to contain {:?}, got {:?}",
        needle,
        res.error
    );
}

pub(super) fn assert_logs_contain_phase(logs: &[BuildLog], phase: BuildPhase) {
    assert!(
        logs.iter().any(|log| log.phase == phase),
        "expected logs to contain phase {:?}, got {:?}",
        phase,
        logs.iter().map(|log| log.phase).collect::<Vec<_>>()
    );
}

pub(super) fn assert_logs_message_contains(logs: &[BuildLog], needle: &str) {
    assert!(
        logs.iter().any(|log| log.message.contains(needle)
            || log
                .details
                .as_deref()
                .is_some_and(|details| details.contains(needle))),
        "expected logs to contain {:?}, got {:?}",
        needle,
        logs.iter()
            .map(|log| (&log.message, log.details.as_deref()))
            .collect::<Vec<_>>()
    );
}
