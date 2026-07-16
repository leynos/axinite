//! Unit tests for heartbeat configuration defaults and builders.

use std::time::Duration;

use super::checklist::{is_effectively_empty, strip_html_comments};
use super::*;

#[test]
fn test_heartbeat_config_defaults() {
    let config = HeartbeatConfig::default();
    assert!(config.enabled);
    assert_eq!(config.interval, Duration::from_secs(30 * 60));
    assert_eq!(config.max_failures, 3);
}

#[test]
fn test_heartbeat_config_builders() {
    let config = HeartbeatConfig::default()
        .with_interval(Duration::from_secs(60))
        .with_notify("user1", "telegram");

    assert_eq!(config.interval, Duration::from_secs(60));
    assert_eq!(config.notify_user_id, Some("user1".to_string()));
    assert_eq!(config.notify_channel, Some("telegram".to_string()));

    let disabled = HeartbeatConfig::default().disabled();
    assert!(!disabled.enabled);
}

// ==================== strip_html_comments ====================

#[test]
fn test_strip_html_comments_no_comments() {
    assert_eq!(strip_html_comments("hello world"), "hello world");
}

#[test]
fn test_strip_html_comments_single() {
    assert_eq!(
        strip_html_comments("before<!-- gone -->after"),
        "beforeafter"
    );
}

#[test]
fn test_strip_html_comments_multiple() {
    let input = "a<!-- 1 -->b<!-- 2 -->c";
    assert_eq!(strip_html_comments(input), "abc");
}

#[test]
fn test_strip_html_comments_multiline() {
    let input = "# Title\n<!-- multi\nline\ncomment -->\nreal content";
    assert_eq!(strip_html_comments(input), "# Title\n\nreal content");
}

#[test]
fn test_strip_html_comments_unclosed() {
    let input = "before<!-- never closed";
    assert_eq!(strip_html_comments(input), "before");
}

// ==================== is_effectively_empty ====================

#[test]
fn test_effectively_empty_empty_string() {
    assert!(is_effectively_empty(""));
}

#[test]
fn test_effectively_empty_whitespace() {
    assert!(is_effectively_empty("   \n\n  \n  "));
}

#[test]
fn test_effectively_empty_headers_only() {
    assert!(is_effectively_empty("# Title\n## Subtitle\n### Section"));
}

#[test]
fn test_effectively_empty_html_comments_only() {
    assert!(is_effectively_empty("<!-- this is a comment -->"));
}

#[test]
fn test_effectively_empty_empty_checkboxes() {
    assert!(is_effectively_empty("# Checklist\n- [ ]\n- [x]"));
}

#[test]
fn test_effectively_empty_bare_list_markers() {
    assert!(is_effectively_empty("-\n*\n-"));
}

#[test]
fn test_effectively_empty_seeded_template() {
    let template = "\
# Heartbeat Checklist

<!-- Keep this file empty to skip heartbeat API calls.
 Add tasks below when you want the agent to check something periodically.

 Example:
 - [ ] Check for unread emails needing a reply
 - [ ] Review today's calendar for upcoming meetings
 - [ ] Check CI build status for main branch
-->";
    assert!(is_effectively_empty(template));
}

#[test]
fn test_effectively_empty_real_checklist() {
    let content = "\
# Heartbeat Checklist

- [ ] Check for unread emails needing a reply
- [ ] Review today's calendar for upcoming meetings";
    assert!(!is_effectively_empty(content));
}

#[test]
fn test_effectively_empty_mixed_real_and_headers() {
    let content = "# Title\n\nDo something important";
    assert!(!is_effectively_empty(content));
}

#[test]
fn test_effectively_empty_comment_plus_real_content() {
    let content = "<!-- comment -->\nActual task here";
    assert!(!is_effectively_empty(content));
}

// ==================== quiet hours ====================

#[test]
fn test_quiet_hours_inside() {
    use chrono::{Timelike, Utc};

    let now_utc = Utc::now();
    let hour = now_utc.hour();
    let start = hour;
    let end = (hour + 1) % 24;

    let config = HeartbeatConfig {
        quiet_hours_start: Some(start),
        quiet_hours_end: Some(end),
        timezone: Some("UTC".to_string()),
        ..HeartbeatConfig::default()
    };
    // Current UTC hour is inside [start, end) by construction
    assert!(config.is_quiet_hours());
}

#[test]
fn test_quiet_hours_outside() {
    use chrono::{Timelike, Utc};

    let now_utc = Utc::now();
    let hour = now_utc.hour();
    let start = (hour + 1) % 24;
    let end = (hour + 2) % 24;

    let config = HeartbeatConfig {
        quiet_hours_start: Some(start),
        quiet_hours_end: Some(end),
        timezone: Some("UTC".to_string()),
        ..HeartbeatConfig::default()
    };
    // Current UTC hour is outside [start, end) by construction
    assert!(!config.is_quiet_hours());
}

#[test]
fn test_quiet_hours_wraparound_excludes_now() {
    use chrono::{Timelike, Utc};

    let now_utc = Utc::now();
    let hour = now_utc.hour();
    // Window covers all hours except the current one
    let start = (hour + 1) % 24;
    let end = hour;

    let config = HeartbeatConfig {
        quiet_hours_start: Some(start),
        quiet_hours_end: Some(end),
        timezone: Some("UTC".to_string()),
        ..HeartbeatConfig::default()
    };
    assert!(!config.is_quiet_hours());
}

#[test]
fn test_quiet_hours_none_configured() {
    let config = HeartbeatConfig::default();
    assert!(!config.is_quiet_hours());
}

#[test]
fn test_quiet_hours_same_start_end() {
    let config = HeartbeatConfig {
        quiet_hours_start: Some(10),
        quiet_hours_end: Some(10),
        timezone: Some("UTC".to_string()),
        ..HeartbeatConfig::default()
    };
    // start == end means zero-width window, should be false
    assert!(!config.is_quiet_hours());
}

#[test]
fn test_spawn_heartbeat_accepts_store_param() {
    // Regression: spawn_heartbeat must accept an optional Database store
    // for persisting heartbeat notifications to a dedicated conversation.
    // Compile-time check: the 7th parameter is `Option<Arc<dyn Database>>`.
    #[allow(clippy::type_complexity)]
    let _fn_ptr: fn(
        HeartbeatConfig,
        HygieneConfig,
        Arc<crate::workspace::Workspace>,
        Arc<dyn crate::llm::LlmProvider>,
        Option<tokio::sync::mpsc::Sender<crate::channels::OutgoingResponse>>,
        Option<Arc<dyn crate::db::Database>>,
    ) -> tokio::task::JoinHandle<()> = spawn_heartbeat;
    let _ = _fn_ptr;
}
