//! Unit tests for routine trigger round-trips and cron scheduling.

use crate::agent::routine::{
    RoutineAction, RoutineGuardrails, RunStatus, Trigger, content_hash, next_cron_fire,
};

#[test]
fn test_trigger_roundtrip() {
    let trigger = Trigger::Cron {
        schedule: "0 9 * * MON-FRI".to_string(),
        timezone: None,
    };
    let json = trigger.to_config_json();
    let parsed = Trigger::from_db("cron", json).expect("parse cron");
    assert!(matches!(parsed, Trigger::Cron { schedule, .. } if schedule == "0 9 * * MON-FRI"));
}

#[test]
fn test_event_trigger_roundtrip() {
    let trigger = Trigger::Event {
        channel: Some("telegram".to_string()),
        pattern: r"deploy\s+\w+".to_string(),
    };
    let json = trigger.to_config_json();
    let parsed = Trigger::from_db("event", json).expect("parse event");
    assert!(matches!(parsed, Trigger::Event { channel, pattern }
        if channel == Some("telegram".to_string()) && pattern == r"deploy\s+\w+"));
}

#[test]
fn test_system_event_trigger_roundtrip() {
    let mut filters = std::collections::HashMap::new();
    filters.insert("repo".to_string(), "nearai/ironclaw".to_string());
    filters.insert("action".to_string(), "opened".to_string());
    let trigger = Trigger::SystemEvent {
        source: "github".to_string(),
        event_type: "issue".to_string(),
        filters: filters.clone(),
    };
    let json = trigger.to_config_json();
    let parsed = Trigger::from_db("system_event", json).expect("parse system_event");
    let Trigger::SystemEvent {
        source,
        event_type,
        filters: f,
    } = parsed
    else {
        panic!("expected SystemEvent trigger");
    };
    assert_eq!(source, "github");
    assert_eq!(event_type, "issue");
    assert_eq!(f, filters);
}

#[test]
fn test_action_lightweight_roundtrip() {
    let action = RoutineAction::Lightweight {
        prompt: "Check PRs".to_string(),
        context_paths: vec!["context/priorities.md".to_string()],
        max_tokens: 2048,
    };
    let json = action.to_config_json();
    let parsed = RoutineAction::from_db("lightweight", json).expect("parse lightweight");
    let RoutineAction::Lightweight {
        prompt,
        context_paths,
        max_tokens,
    } = parsed
    else {
        panic!("expected Lightweight action");
    };
    assert_eq!(prompt, "Check PRs");
    assert_eq!(context_paths.len(), 1);
    assert_eq!(max_tokens, 2048);
}

#[test]
fn test_action_full_job_roundtrip() {
    let action = RoutineAction::FullJob {
        title: "Deploy review".to_string(),
        description: "Review and deploy pending changes".to_string(),
        max_iterations: 5,
        tool_permissions: vec!["shell".to_string()],
    };
    let json = action.to_config_json();
    let parsed = RoutineAction::from_db("full_job", json).expect("parse full_job");
    let RoutineAction::FullJob {
        title,
        max_iterations,
        tool_permissions,
        ..
    } = parsed
    else {
        panic!("expected FullJob action");
    };
    assert_eq!(title, "Deploy review");
    assert_eq!(max_iterations, 5);
    assert_eq!(tool_permissions, vec!["shell".to_string()]);
}

#[test]
fn test_run_status_display_parse() {
    for status in [
        RunStatus::Running,
        RunStatus::Ok,
        RunStatus::Attention,
        RunStatus::Failed,
    ] {
        let s = status.to_string();
        let parsed: RunStatus = s.parse().expect("parse status");
        assert_eq!(parsed, status);
    }
}

#[test]
fn test_content_hash_deterministic() {
    let h1 = content_hash("deploy production");
    let h2 = content_hash("deploy production");
    assert_eq!(h1, h2);

    let h3 = content_hash("deploy staging");
    assert_ne!(h1, h3);
}

#[test]
fn test_next_cron_fire_valid() {
    // Every minute should always have a next fire
    let next = next_cron_fire("* * * * * *", None).expect("valid cron");
    assert!(next.is_some());
}

#[test]
fn test_next_cron_fire_invalid() {
    let result = next_cron_fire("not a cron", None);
    assert!(result.is_err());
}

#[test]
fn test_trigger_cron_timezone_roundtrip() {
    let trigger = Trigger::Cron {
        schedule: "0 9 * * MON-FRI".to_string(),
        timezone: Some("America/New_York".to_string()),
    };
    let json = trigger.to_config_json();
    let parsed = Trigger::from_db("cron", json).expect("parse cron");
    assert!(matches!(parsed, Trigger::Cron { schedule, timezone }
            if schedule == "0 9 * * MON-FRI"
            && timezone.as_deref() == Some("America/New_York")));
}

#[test]
fn test_trigger_cron_no_timezone_backward_compat() {
    let json = serde_json::json!({"schedule": "0 9 * * *"});
    let parsed = Trigger::from_db("cron", json).expect("parse cron");
    assert!(matches!(parsed, Trigger::Cron { timezone, .. } if timezone.is_none()));
}

#[test]
fn test_trigger_cron_invalid_timezone_coerced_to_none() {
    let json = serde_json::json!({"schedule": "0 9 * * *", "timezone": "Fake/Zone"});
    let parsed = Trigger::from_db("cron", json).expect("parse cron");
    assert!(
        matches!(parsed, Trigger::Cron { timezone, .. } if timezone.is_none()),
        "invalid timezone should be coerced to None"
    );
}

#[test]
fn test_next_cron_fire_with_timezone() {
    let next_utc = next_cron_fire("0 0 9 * * * *", None)
        .expect("valid cron")
        .expect("has next");
    let next_est = next_cron_fire("0 0 9 * * * *", Some("America/New_York"))
        .expect("valid cron")
        .expect("has next");
    // EST is UTC-5 (or EDT UTC-4), so the UTC result should differ
    assert_ne!(next_utc, next_est, "timezone should shift the fire time");
}

#[test]
fn test_guardrails_default() {
    let g = RoutineGuardrails::default();
    assert_eq!(g.cooldown.as_secs(), 300);
    assert_eq!(g.max_concurrent, 1);
    assert!(g.dedup_window.is_none());
}

#[test]
fn test_trigger_type_tag() {
    assert_eq!(
        Trigger::Cron {
            schedule: String::new(),
            timezone: None,
        }
        .type_tag(),
        "cron"
    );
    assert_eq!(
        Trigger::Event {
            channel: None,
            pattern: String::new()
        }
        .type_tag(),
        "event"
    );
    assert_eq!(
        Trigger::SystemEvent {
            source: String::new(),
            event_type: String::new(),
            filters: std::collections::HashMap::new(),
        }
        .type_tag(),
        "system_event"
    );
    assert_eq!(Trigger::Manual.type_tag(), "manual");
}
