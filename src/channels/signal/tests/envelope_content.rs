//! Tests for envelope content handling: UUID senders, metadata fields,
//! attachment/text combinations, display names, thread IDs, and timestamps.

use super::*;

#[test]
fn process_envelope_uuid_sender_dm() -> Result<(), ChannelError> {
    let uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    let ch = SignalChannel::new(config)?;

    let env = Envelope {
        source: Some(uuid.to_string()),
        source_number: None,
        source_name: Some("Privacy User".to_string()),
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("Hello from privacy user".to_string()),
            timestamp: Some(1_700_000_000_000),
            group_info: None,
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(1_700_000_000_000),
    };
    let (msg, target) = ch.process_envelope(&env).unwrap();
    assert_eq!(msg.user_id, uuid);
    assert_eq!(msg.user_name.as_deref(), Some("Privacy User"));
    assert_eq!(msg.content, "Hello from privacy user");
    assert_eq!(target, uuid);

    // Verify reply routing: UUID sender in DM should route as Direct.
    let parsed = SignalChannel::parse_recipient_target(&target);
    assert_eq!(parsed, RecipientTarget::Direct(uuid.to_string()));
    Ok(())
}

#[test]
fn process_envelope_uuid_sender_in_group() -> Result<(), ChannelError> {
    let uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let mut config = make_config_with_allowed_group("testgroup");
    config.ignore_attachments = false;
    config.ignore_stories = false;
    let ch = SignalChannel::new(config)?;

    let env = Envelope {
        source: Some(uuid.to_string()),
        source_number: None,
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("Group msg from privacy user".to_string()),
            timestamp: Some(1_700_000_000_000),
            group_info: Some(GroupInfo {
                group_id: Some("testgroup".to_string()),
            }),
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(1_700_000_000_000),
    };
    let (msg, target) = ch.process_envelope(&env).unwrap();
    assert_eq!(msg.user_id, uuid);
    assert_eq!(target, "group:testgroup");
    // Groups now use deterministic UUID derived from group ID
    let expected_thread_id = SignalChannel::thread_id_from_identifier("group:testgroup");
    assert_eq!(msg.thread_id, Some(expected_thread_id));

    // Verify reply routing: group message should still route as Group.
    let parsed = SignalChannel::parse_recipient_target(&target);
    assert_eq!(parsed, RecipientTarget::Group("testgroup".to_string()));
    Ok(())
}

// ── metadata assertion tests ────────────────────────────────────

#[test]
fn process_envelope_metadata_has_signal_fields() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    let env = make_envelope(Some("+1111111111"), Some("Hello!"));
    let (msg, _) = ch.process_envelope(&env).unwrap();
    assert_eq!(msg.metadata["signal_sender"], "+1111111111");
    assert_eq!(msg.metadata["signal_target"], "+1111111111");
    assert_eq!(msg.metadata["signal_timestamp"], 1_700_000_000_000_u64);
    Ok(())
}

#[test]
fn process_envelope_metadata_group_target() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    config.allow_from_groups = vec!["*".to_string()];
    config.group_policy = "allowlist".to_string();
    let ch = SignalChannel::new(config)?;

    let env = Envelope {
        source: Some("+2222222222".to_string()),
        source_number: Some("+2222222222".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("In the group".to_string()),
            timestamp: Some(1_700_000_000_000),
            group_info: Some(GroupInfo {
                group_id: Some("mygroup".to_string()),
            }),
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(1_700_000_000_000),
    };
    let (msg, _) = ch.process_envelope(&env).unwrap();
    assert_eq!(msg.metadata["signal_target"], "group:mygroup");
    assert_eq!(msg.metadata["signal_sender"], "+2222222222");
    Ok(())
}

// ── attachment-with-text tests ──────────────────────────────────

#[test]
fn process_envelope_attachment_with_text_not_skipped() -> Result<(), ChannelError> {
    // Even with ignore_attachments=true, messages that have BOTH text
    // and attachments should be processed (only attachment-only are skipped).
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    config.ignore_attachments = true;
    let ch = SignalChannel::new(config)?;

    let env = Envelope {
        source: Some("+1111111111".to_string()),
        source_number: Some("+1111111111".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("Check this out".to_string()),
            timestamp: Some(1_700_000_000_000),
            group_info: None,
            attachments: Some(vec![serde_json::json!({"contentType": "image/png"})]),
        }),
        story_message: None,
        timestamp: Some(1_700_000_000_000),
    };
    let result = ch.process_envelope(&env);
    assert!(
        result.is_some(),
        "Message with text + attachment should not be skipped"
    );
    let (msg, _) = result.unwrap();
    assert_eq!(msg.content, "Check this out");
    Ok(())
}

#[test]
fn process_envelope_attachment_only_not_skipped_when_ignore_disabled() -> Result<(), ChannelError> {
    // With ignore_attachments=false, attachment-only messages should be
    // processed with the "[Attachment]" placeholder text.
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    config.ignore_attachments = false;
    let ch = SignalChannel::new(config)?;

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
    // With ignore_attachments=false, attachment-only messages are now
    // processed with a placeholder "[Attachment]" text.
    let result = ch.process_envelope(&env);
    assert!(
        result.is_some(),
        "Attachment-only should be processed when ignore_attachments=false"
    );
    let (msg, _) = result.unwrap();
    assert_eq!(msg.content, "[Attachment]");
    Ok(())
}

// ── source_name / display name tests ────────────────────────────

#[test]
fn process_envelope_source_name_sets_user_name() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    let ch = SignalChannel::new(config)?;

    let env = Envelope {
        source: Some("+3333333333".to_string()),
        source_number: Some("+3333333333".to_string()),
        source_name: Some("Alice".to_string()),
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("Hey".to_string()),
            timestamp: Some(1_700_000_000_000),
            group_info: None,
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(1_700_000_000_000),
    };
    let (msg, _) = ch.process_envelope(&env).unwrap();
    assert_eq!(msg.user_name.as_deref(), Some("Alice"));
    Ok(())
}

#[test]
fn process_envelope_empty_source_name_not_set() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    let ch = SignalChannel::new(config)?;

    let env = Envelope {
        source: Some("+3333333333".to_string()),
        source_number: Some("+3333333333".to_string()),
        source_name: Some("".to_string()),
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("Hey".to_string()),
            timestamp: Some(1_700_000_000_000),
            group_info: None,
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(1_700_000_000_000),
    };
    let (msg, _) = ch.process_envelope(&env).unwrap();
    assert!(
        msg.user_name.is_none(),
        "Empty source_name should not set user_name"
    );
    Ok(())
}

#[test]
fn process_envelope_no_source_name_not_set() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    let env = make_envelope(Some("+1111111111"), Some("hi"));
    let (msg, _) = ch.process_envelope(&env).unwrap();
    assert!(msg.user_name.is_none());
    Ok(())
}

// ── thread_id tests ─────────────────────────────────────────────────────────────────

#[test]
fn process_envelope_dm_sets_thread_id_to_uuid() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    let env = make_envelope(Some("+1111111111"), Some("DM"));
    let (msg, _) = ch.process_envelope(&env).unwrap();
    // DMs now set thread_id to a deterministic UUID derived from phone number
    let expected_thread_id = SignalChannel::thread_id_from_identifier("+1111111111");
    assert_eq!(
        msg.thread_id,
        Some(expected_thread_id),
        "DMs should set thread_id to UUID"
    );
    Ok(())
}

#[test]
fn process_envelope_group_sets_thread_id_to_uuid() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    config.allow_from_groups = vec!["*".to_string()];
    config.group_policy = "allowlist".to_string();
    let ch = SignalChannel::new(config)?;

    let env = Envelope {
        source: Some("+1111111111".to_string()),
        source_number: Some("+1111111111".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("Group msg".to_string()),
            timestamp: Some(1_700_000_000_000),
            group_info: Some(GroupInfo {
                group_id: Some("grp999".to_string()),
            }),
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(1_700_000_000_000),
    };
    let (msg, _) = ch.process_envelope(&env).unwrap();
    // Groups now set thread_id to a deterministic UUID derived from group ID
    let expected_thread_id = SignalChannel::thread_id_from_identifier("group:grp999");
    assert_eq!(
        msg.thread_id,
        Some(expected_thread_id),
        "Groups should set thread_id to UUID"
    );
    Ok(())
}
