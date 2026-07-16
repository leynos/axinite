//! Tests for reply-target derivation, recipient-target parsing, and
//! identifier helpers (E.164, UUID, deterministic thread IDs, sender).

use super::*;

#[test]
fn reply_target_dm() {
    let dm = DataMessage {
        message: Some("hi".to_string()),
        timestamp: Some(1000),
        group_info: None,
        attachments: None,
    };
    assert_eq!(
        SignalChannel::reply_target(&dm, "+1111111111"),
        "+1111111111"
    );
}

#[test]
fn reply_target_group() {
    let group = DataMessage {
        message: Some("hi".to_string()),
        timestamp: Some(1000),
        group_info: Some(GroupInfo {
            group_id: Some("group123".to_string()),
        }),
        attachments: None,
    };
    assert_eq!(
        SignalChannel::reply_target(&group, "+1111111111"),
        "group:group123"
    );
}

#[test]
fn parse_recipient_target_e164_is_direct() {
    assert_eq!(
        SignalChannel::parse_recipient_target("+1234567890"),
        RecipientTarget::Direct("+1234567890".to_string())
    );
}

#[test]
fn parse_recipient_target_prefixed_group_is_group() {
    assert_eq!(
        SignalChannel::parse_recipient_target("group:abc123"),
        RecipientTarget::Group("abc123".to_string())
    );
}

#[test]
fn parse_recipient_target_uuid_is_direct() {
    let uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    assert_eq!(
        SignalChannel::parse_recipient_target(uuid),
        RecipientTarget::Direct(uuid.to_string())
    );
}

#[test]
fn parse_recipient_target_non_e164_plus_is_group() {
    assert_eq!(
        SignalChannel::parse_recipient_target("+abc123"),
        RecipientTarget::Group("+abc123".to_string())
    );
}

#[test]
fn is_uuid_valid() {
    assert!(SignalChannel::is_uuid(
        "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
    ));
    assert!(SignalChannel::is_uuid(
        "00000000-0000-0000-0000-000000000000"
    ));
}

#[test]
fn is_uuid_invalid() {
    assert!(!SignalChannel::is_uuid("+1234567890"));
    assert!(!SignalChannel::is_uuid("not-a-uuid"));
    assert!(!SignalChannel::is_uuid("group:abc123"));
    assert!(!SignalChannel::is_uuid(""));
}

#[test]
fn thread_id_from_identifier_is_deterministic() {
    let id1 = SignalChannel::thread_id_from_identifier("+1234567890");
    let id2 = SignalChannel::thread_id_from_identifier("+1234567890");
    assert_eq!(id1, id2, "same input should produce same UUID");
}

#[test]
fn thread_id_from_identifier_is_valid_uuid() {
    let id = SignalChannel::thread_id_from_identifier("+1234567890");
    assert!(Uuid::parse_str(&id).is_ok(), "should be a valid UUID");
}

#[test]
fn thread_id_from_identifier_different_inputs() {
    let id1 = SignalChannel::thread_id_from_identifier("+1234567890");
    let id2 = SignalChannel::thread_id_from_identifier("+9876543210");
    assert_ne!(id1, id2, "different inputs should produce different UUIDs");
}

#[test]
fn sender_prefers_source_number() {
    let env = Envelope {
        source: Some("uuid-123".to_string()),
        source_number: Some("+1111111111".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: None,
        story_message: None,
        timestamp: Some(1000),
    };
    assert_eq!(SignalChannel::sender(&env), Some("+1111111111".to_string()));
}

#[test]
fn sender_falls_back_to_source() {
    let env = Envelope {
        source: Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string()),
        source_number: None,
        source_name: None,
        source_uuid: None,
        data_message: None,
        story_message: None,
        timestamp: Some(1000),
    };
    assert_eq!(
        SignalChannel::sender(&env),
        Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string())
    );
}

#[test]
fn sender_none_when_both_missing() {
    let env = Envelope {
        source: None,
        source_number: None,
        source_name: None,
        source_uuid: None,
        data_message: None,
        story_message: None,
        timestamp: None,
    };
    assert_eq!(SignalChannel::sender(&env), None);
}

// ── is_e164 tests ───────────────────────────────────────────────

#[test]
fn is_e164_valid_numbers() {
    assert!(SignalChannel::is_e164("+12345678901"));
    assert!(SignalChannel::is_e164("+1234567")); // min 7 digits after +
    assert!(SignalChannel::is_e164("+123456789012345")); // max 15 digits
}

#[test]
fn is_e164_invalid_numbers() {
    assert!(!SignalChannel::is_e164("12345678901")); // no +
    assert!(!SignalChannel::is_e164("+1")); // too short (1 digit)
    assert!(!SignalChannel::is_e164("+1234567890123456")); // too long (16 digits)
    assert!(!SignalChannel::is_e164("+abc123")); // non-digit
    assert!(!SignalChannel::is_e164("")); // empty
    assert!(!SignalChannel::is_e164("+")); // plus only
}
