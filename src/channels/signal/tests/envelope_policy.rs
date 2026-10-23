//! Tests for DM/group policy enforcement in `process_envelope`, including
//! pairing-policy behaviour and story/attachment drop rules.

use super::*;

#[test]
fn process_envelope_dm_accepted_with_empty_allow_from_groups() {
    // Empty allow_from_groups = DMs only. DMs should be accepted.
    let ch = make_channel();
    let env = make_envelope(Some("+1111111111"), Some("Hello!"));
    assert!(ch.process_envelope(&env).is_some());
}

#[test]
fn process_envelope_group_denied_with_empty_allow_from_groups() {
    // Empty allow_from_groups = DMs only. Group messages should be denied.
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    let ch = make_channel_from_config(config);

    let env = Envelope {
        source: Some("+1111111111".to_string()),
        source_number: Some("+1111111111".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("hi".to_string()),
            timestamp: Some(1000),
            group_info: Some(GroupInfo {
                group_id: Some("group123".to_string()),
            }),
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(1000),
    };
    assert!(ch.process_envelope(&env).is_none());
}

#[test]
fn process_envelope_group_accepted_when_in_allow_from_groups() {
    let ch = make_channel_with_allowed_group("group123");

    let env = Envelope {
        source: Some("+1111111111".to_string()),
        source_number: Some("+1111111111".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("hi".to_string()),
            timestamp: Some(1000),
            group_info: Some(GroupInfo {
                group_id: Some("group123".to_string()),
            }),
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(1000),
    };
    assert!(ch.process_envelope(&env).is_some());

    // Different group should be denied.
    let env2 = Envelope {
        source: Some("+1111111111".to_string()),
        source_number: Some("+1111111111".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("hi".to_string()),
            timestamp: Some(1000),
            group_info: Some(GroupInfo {
                group_id: Some("other_group".to_string()),
            }),
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(1000),
    };
    assert!(ch.process_envelope(&env2).is_none());
}

#[test]
fn process_envelope_valid_dm() {
    let ch = make_channel();
    let env = make_envelope(Some("+1111111111"), Some("Hello!"));
    let (msg, target) = ch
        .process_envelope(&env)
        .expect("envelope should be accepted and processed");
    assert_eq!(msg.content, "Hello!");
    assert_eq!(msg.user_id, "+1111111111");
    assert_eq!(msg.channel, "signal");
    assert_eq!(target, "+1111111111");
}

#[tokio::test]
async fn process_envelope_pairing_accepts_already_paired_sender() {
    let _pairing_guard =
        SignalPairingStoreGuard::install().expect("pairing store guard should install");
    let mut config = make_config();
    config.allow_from = vec![];
    config.dm_policy = "pairing".to_string();
    let ch = make_channel_from_config(config);

    let sender = "+9999999998";
    let store = SignalChannel::pairing_store();
    let request = store
        .upsert_request("signal", sender, None)
        .expect("failed to create upsert_request for signal");
    store
        .approve("signal", &request.code)
        .expect("failed to approve signal request");

    let env = make_envelope(Some(sender), Some("Hello!"));
    assert!(ch.process_envelope(&env).is_some());
}

#[test]
fn process_envelope_denied_sender() {
    let ch = make_channel();
    let env = make_envelope(Some("+9999999999"), Some("Hello!"));
    assert!(ch.process_envelope(&env).is_none());
}

#[tokio::test]
async fn process_envelope_pairing_creates_request_for_unpaired_sender() {
    let _pairing_guard =
        SignalPairingStoreGuard::install().expect("pairing store guard should install");
    let mut config = make_config();
    config.allow_from = vec![];
    config.dm_policy = "pairing".to_string();
    let ch = make_channel_from_config(config);

    let sender = "+9999999997";
    let env = make_envelope(Some(sender), Some("Hello!"));
    assert!(ch.process_envelope(&env).is_none());

    let pending = SignalChannel::pairing_store()
        .list_pending("signal")
        .expect("failed to retrieve pending signal requests");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, sender);
}

#[test]
fn process_envelope_empty_message() {
    let ch = make_channel();
    let env = make_envelope(Some("+1111111111"), Some(""));
    assert!(ch.process_envelope(&env).is_none());
}

#[test]
fn process_envelope_no_data_message() {
    let ch = make_channel();
    let env = make_envelope(Some("+1111111111"), None);
    assert!(ch.process_envelope(&env).is_none());
}

#[test]
fn process_envelope_skips_stories() {
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    config.ignore_stories = true;
    let ch = make_channel_from_config(config);
    let mut env = make_envelope(Some("+1111111111"), Some("story text"));
    env.story_message = Some(serde_json::json!({}));
    assert!(ch.process_envelope(&env).is_none());
}

#[test]
fn process_envelope_skips_attachment_only() {
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    config.ignore_attachments = true;
    let ch = make_channel_from_config(config);
    let env = Envelope {
        source: Some("+1111111111".to_string()),
        source_number: Some("+1111111111".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: None,
            timestamp: Some(1_700_000_000_000),
            group_info: None,
            attachments: Some(vec![serde_json::json!({"contentType": "image/png"})]),
        }),
        story_message: None,
        timestamp: Some(1_700_000_000_000),
    };
    assert!(ch.process_envelope(&env).is_none());
}

#[test]
fn process_envelope_group_not_in_allow_from_groups() {
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    config.allow_from_groups = vec!["allowed_group".to_string()];
    let ch = make_channel_from_config(config);

    let env = Envelope {
        source: Some("+1111111111".to_string()),
        source_number: Some("+1111111111".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("Hi".to_string()),
            timestamp: Some(1_700_000_000_000),
            group_info: Some(GroupInfo {
                group_id: Some("other_group".to_string()),
            }),
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(1_700_000_000_000),
    };
    assert!(ch.process_envelope(&env).is_none());
}

// ── stories behaviour tests ──────────────────────────────────────

#[test]
fn process_envelope_stories_not_skipped_when_disabled() {
    // With ignore_stories=false, story messages with a data_message
    // should still be processed.
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    config.ignore_stories = false;
    let ch = make_channel_from_config(config);

    let env = Envelope {
        source: Some("+1111111111".to_string()),
        source_number: Some("+1111111111".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("story with text".to_string()),
            timestamp: Some(1_700_000_000_000),
            group_info: None,
            attachments: None,
        }),
        story_message: Some(serde_json::json!({})),
        timestamp: Some(1_700_000_000_000),
    };
    let result = ch.process_envelope(&env);
    assert!(
        result.is_some(),
        "Stories should not be skipped when ignore_stories=false"
    );
}
