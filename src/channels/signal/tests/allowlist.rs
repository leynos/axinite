//! Tests for sender and group allowlist matching, including wildcard and
//! `uuid:` prefix normalization.

use super::*;

#[test]
fn wildcard_allows_anyone() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    let ch = SignalChannel::new(config)?;
    assert!(ch.is_sender_allowed("+9999999999"));
    Ok(())
}

#[test]
fn specific_sender_allowed() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    assert!(ch.is_sender_allowed("+1111111111"));
    Ok(())
}

#[test]
fn unknown_sender_denied() -> Result<(), ChannelError> {
    let ch = make_channel()?;
    assert!(!ch.is_sender_allowed("+9999999999"));
    Ok(())
}

#[test]
fn empty_allowlist_denies_all() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from = vec![];
    let ch = SignalChannel::new(config)?;
    assert!(!ch.is_sender_allowed("+1111111111"));
    Ok(())
}

#[test]
fn uuid_prefix_in_allowlist() -> Result<(), ChannelError> {
    let uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let mut config = make_config();
    config.allow_from = vec![format!("uuid:{uuid}")];
    let ch = SignalChannel::new(config)?;
    assert!(ch.is_sender_allowed(uuid));
    // Should not match phone numbers.
    assert!(!ch.is_sender_allowed("+1111111111"));
    Ok(())
}

#[test]
fn bare_uuid_in_allowlist() -> Result<(), ChannelError> {
    let uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let mut config = make_config();
    config.allow_from = vec![uuid.to_string()];
    let ch = SignalChannel::new(config)?;
    assert!(ch.is_sender_allowed(uuid));
    Ok(())
}

#[test]
fn group_allowlist_filtering() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    config.allow_from_groups = vec!["group123".to_string()];
    let ch = SignalChannel::new(config)?;
    assert!(ch.is_group_allowed("group123"));
    assert!(!ch.is_group_allowed("other_group"));
    Ok(())
}

#[test]
fn group_allowlist_wildcard() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from_groups = vec!["*".to_string()];
    let ch = SignalChannel::new(config)?;
    assert!(ch.is_group_allowed("any_group"));
    Ok(())
}

#[test]
fn group_allowlist_empty_denies_all() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from_groups = vec![];
    let ch = SignalChannel::new(config)?;
    assert!(!ch.is_group_allowed("any_group"));
    Ok(())
}

#[test]
fn normalize_allow_entry_strips_uuid_prefix() {
    assert_eq!(
        SignalChannel::normalize_allow_entry("uuid:abc-123"),
        "abc-123"
    );
    assert_eq!(
        SignalChannel::normalize_allow_entry("+1234567890"),
        "+1234567890"
    );
    assert_eq!(SignalChannel::normalize_allow_entry("*"), "*");
}

// ── config edge cases ───────────────────────────────────────────

#[test]
fn multiple_allow_from() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from = vec![
        "+1111111111".to_string(),
        "+2222222222".to_string(),
        "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string(),
    ];
    let ch = SignalChannel::new(config)?;
    assert!(ch.is_sender_allowed("+1111111111"));
    assert!(ch.is_sender_allowed("+2222222222"));
    assert!(ch.is_sender_allowed("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
    assert!(!ch.is_sender_allowed("+9999999999"));
    Ok(())
}

#[test]
fn multiple_allow_from_groups() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from_groups = vec!["group_a".to_string(), "group_b".to_string()];
    let ch = SignalChannel::new(config)?;
    assert!(ch.is_group_allowed("group_a"));
    assert!(ch.is_group_allowed("group_b"));
    assert!(!ch.is_group_allowed("group_c"));
    Ok(())
}

#[test]
fn uuid_prefix_normalization_in_allowlist() -> Result<(), ChannelError> {
    let uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    let mut config = make_config();
    config.allow_from = vec![format!("uuid:{uuid}"), "+1111111111".to_string()];
    let ch = SignalChannel::new(config)?;
    // uuid:-prefixed entry should match bare UUID sender.
    assert!(ch.is_sender_allowed(uuid));
    // Phone numbers still work alongside UUID entries.
    assert!(ch.is_sender_allowed("+1111111111"));
    // Non-matching should fail.
    assert!(!ch.is_sender_allowed("+9999999999"));
    Ok(())
}
