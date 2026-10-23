//! Status-assertion helpers for the dispatcher pipeline tests.
//!
//! Each `assert_*` wrapper is a thin `#[track_caller]` layer over the pure
//! `has_status` query so failures point at the calling test.

use crate::channels::StatusUpdate;

/// Report whether any recorded status satisfies `predicate`.
fn has_status(statuses: &[StatusUpdate], predicate: impl Fn(&StatusUpdate) -> bool) -> bool {
    statuses.iter().any(predicate)
}

#[track_caller]
pub(super) fn assert_thinking_status(statuses: &[StatusUpdate], expected: &str) {
    assert!(
        has_status(
            statuses,
            |status| matches!(status, StatusUpdate::Thinking(message) if message == expected)
        ),
        "expected Thinking status `{expected}`, got: {statuses:?}"
    );
}

#[track_caller]
pub(super) fn assert_tool_result_status(statuses: &[StatusUpdate], tool_name: &str) {
    assert!(
        has_status(statuses, |status| matches!(
            status,
            StatusUpdate::ToolResult { name, preview }
                if name == tool_name && !preview.is_empty()
        )),
        "expected non-empty ToolResult preview for `{tool_name}`, got: {statuses:?}"
    );
}

#[track_caller]
pub(super) fn assert_tool_completed_status(statuses: &[StatusUpdate], tool_name: &str) {
    assert!(
        has_status(statuses, |status| matches!(
            status,
            StatusUpdate::ToolCompleted { name, success, .. }
                if name == tool_name && *success
        )),
        "expected successful ToolCompleted for `{tool_name}`, got: {statuses:?}"
    );
}

#[track_caller]
pub(super) fn assert_tool_started_status(statuses: &[StatusUpdate], tool_name: &str) {
    assert!(
        has_status(
            statuses,
            |status| matches!(status, StatusUpdate::ToolStarted { name } if name == tool_name)
        ),
        "expected ToolStarted for `{tool_name}`, got: {statuses:?}"
    );
}
