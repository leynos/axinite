//! Tests for timestamp selection in `process_envelope`: data-message
//! priority, envelope fallback, and generated timestamps.

use super::*;

#[test]
fn process_envelope_uses_data_message_timestamp() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    let ch = SignalChannel::new(config)?;

    let env = Envelope {
        source: Some("+1111111111".to_string()),
        source_number: Some("+1111111111".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("hi".to_string()),
            timestamp: Some(9999),
            group_info: None,
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(1111),
    };
    let (msg, _) = ch.process_envelope(&env).unwrap();
    // data_message timestamp takes priority.
    assert_eq!(msg.metadata["signal_timestamp"], 9999);
    Ok(())
}

#[test]
fn process_envelope_falls_back_to_envelope_timestamp() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    let ch = SignalChannel::new(config)?;

    let env = Envelope {
        source: Some("+1111111111".to_string()),
        source_number: Some("+1111111111".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("hi".to_string()),
            timestamp: None,
            group_info: None,
            attachments: None,
        }),
        story_message: None,
        timestamp: Some(7777),
    };
    let (msg, _) = ch.process_envelope(&env).unwrap();
    assert_eq!(msg.metadata["signal_timestamp"], 7777);
    Ok(())
}

#[test]
fn process_envelope_generates_timestamp_when_missing() -> Result<(), ChannelError> {
    let mut config = make_config();
    config.allow_from = vec!["*".to_string()];
    let ch = SignalChannel::new(config)?;

    let env = Envelope {
        source: Some("+1111111111".to_string()),
        source_number: Some("+1111111111".to_string()),
        source_name: None,
        source_uuid: None,
        data_message: Some(DataMessage {
            message: Some("hi".to_string()),
            timestamp: None,
            group_info: None,
            attachments: None,
        }),
        story_message: None,
        timestamp: None,
    };
    let (msg, _) = ch.process_envelope(&env).unwrap();
    // Should generate a timestamp (current time in millis), just verify it's positive.
    let ts = msg.metadata["signal_timestamp"].as_u64().unwrap();
    assert!(ts > 0, "Generated timestamp should be positive");
    Ok(())
}
