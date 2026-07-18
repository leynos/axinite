//! Unit tests for the log broadcaster and web log layer.

use super::*;

#[test]
fn test_log_broadcaster_creation() {
    let broadcaster = LogBroadcaster::new();
    // Should not panic with no receivers
    broadcaster.send(LogEntry {
        level: "INFO".to_string(),
        target: "test".to_string(),
        message: "hello".to_string(),
        timestamp: "2024-01-01T00:00:00.000Z".to_string(),
    });
}

#[test]
fn test_log_broadcaster_subscribe() {
    let broadcaster = LogBroadcaster::new();
    let mut rx = broadcaster.subscribe();

    broadcaster.send(LogEntry {
        level: "WARN".to_string(),
        target: "axinite::test".to_string(),
        message: "test warning".to_string(),
        timestamp: "2024-01-01T00:00:00.000Z".to_string(),
    });

    let entry = rx.try_recv().expect("should receive entry");
    assert_eq!(entry.level, "WARN");
    assert_eq!(entry.message, "test warning");
}

#[test]
fn test_log_entry_serialization() {
    let entry = LogEntry {
        level: "ERROR".to_string(),
        target: "axinite::agent".to_string(),
        message: "something broke".to_string(),
        timestamp: "2024-01-01T00:00:00.000Z".to_string(),
    };
    let json = serde_json::to_string(&entry).expect("should serialize");
    assert!(json.contains("\"level\":\"ERROR\""));
    assert!(json.contains("something broke"));
}

#[test]
fn test_recent_entries_buffer() {
    let broadcaster = LogBroadcaster::new();

    for i in 0..5 {
        broadcaster.send(LogEntry {
            level: "INFO".to_string(),
            target: "test".to_string(),
            message: format!("msg {}", i),
            timestamp: "2024-01-01T00:00:00.000Z".to_string(),
        });
    }

    let recent = broadcaster.recent_entries();
    assert_eq!(recent.len(), 5);
    assert_eq!(recent[0].message, "msg 0");
    assert_eq!(recent[4].message, "msg 4");
}

#[test]
fn test_recent_entries_cap() {
    let broadcaster = LogBroadcaster::new();

    // Overflow the buffer
    for i in 0..(HISTORY_CAP + 50) {
        broadcaster.send(LogEntry {
            level: "INFO".to_string(),
            target: "test".to_string(),
            message: format!("msg {}", i),
            timestamp: "2024-01-01T00:00:00.000Z".to_string(),
        });
    }

    let recent = broadcaster.recent_entries();
    assert_eq!(recent.len(), HISTORY_CAP);
    // Oldest should be msg 50 (first 50 evicted)
    assert_eq!(recent[0].message, "msg 50");
}

#[test]
fn test_recent_entries_available_without_subscribers() {
    let broadcaster = LogBroadcaster::new();
    // No subscribe() call, just send
    broadcaster.send(LogEntry {
        level: "INFO".to_string(),
        target: "test".to_string(),
        message: "before anyone listened".to_string(),
        timestamp: "2024-01-01T00:00:00.000Z".to_string(),
    });

    let recent = broadcaster.recent_entries();
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].message, "before anyone listened");
}

#[test]
fn test_message_visitor_finish_message_only() {
    let v = MessageVisitor {
        message: "hello world".to_string(),
        fields: vec![],
    };
    assert_eq!(v.finish(), "hello world");
}

#[test]
fn test_message_visitor_finish_with_fields() {
    let v = MessageVisitor {
        message: "Request completed".to_string(),
        fields: vec![
            "url=http://localhost:8080".to_string(),
            "status=200".to_string(),
        ],
    };
    let result = v.finish();
    assert_eq!(
        result,
        "Request completed url=http://localhost:8080 status=200"
    );
}

#[test]
fn test_message_visitor_finish_empty() {
    let v = MessageVisitor::new();
    assert_eq!(v.finish(), "");
}

#[test]
fn test_broadcaster_has_leak_detector() {
    let broadcaster = LogBroadcaster::new();
    // Verify the leak detector is initialized with default patterns
    assert!(broadcaster.leak_detector.pattern_count() > 0);
}

#[test]
fn test_leak_detector_scrubs_api_key_in_log() {
    let detector = crate::safety::LeakDetector::new();
    let msg = "Connecting with token sk-proj-test1234567890abcdefghij";
    let result = detector.scan_and_clean(msg);
    // Should be blocked (OpenAI key pattern)
    assert!(result.is_err());
}

#[test]
fn test_leak_detector_passes_clean_log() {
    let detector = crate::safety::LeakDetector::new();
    let msg = "Request completed status=200 url=https://api.example.com/data";
    let result = detector.scan_and_clean(msg);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), msg);
}
