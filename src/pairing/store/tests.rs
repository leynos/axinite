//! Unit tests for the pairing store and channel key sanitization.

use super::codes::{PAIRING_ALPHABET, PAIRING_CODE_LENGTH, random_code};
use super::paths::safe_channel_key;
use super::*;
use tempfile::TempDir;

#[test]
fn test_safe_channel_key() {
    assert_eq!(safe_channel_key("telegram").unwrap(), "telegram");
    assert_eq!(safe_channel_key("Telegram").unwrap(), "telegram");
    safe_channel_key("").unwrap_err();
}

#[test]
fn test_random_code() {
    let c = random_code();
    assert_eq!(c.len(), PAIRING_CODE_LENGTH);
    assert!(c.chars().all(|c| PAIRING_ALPHABET.contains(&(c as u8))));
}

fn test_store() -> (PairingStore, TempDir) {
    let dir = TempDir::new().unwrap();
    let store = PairingStore::with_base_dir(dir.path().to_path_buf());
    (store, dir)
}

#[test]
fn test_list_pending_empty() {
    let (store, _) = test_store();
    let requests = store.list_pending("telegram").unwrap();
    assert!(requests.is_empty());
}

#[test]
fn test_upsert_request_creates_new() {
    let (store, _) = test_store();
    let result = store
        .upsert_request(
            "telegram",
            "user123",
            Some(serde_json::json!({"chat_id": 456})),
        )
        .unwrap();
    assert!(result.created);
    assert_eq!(result.code.len(), PAIRING_CODE_LENGTH);
    assert!(
        result
            .code
            .chars()
            .all(|c| PAIRING_ALPHABET.contains(&(c as u8)))
    );
}

#[test]
fn test_upsert_request_updates_existing() {
    let (store, _) = test_store();
    let r1 = store.upsert_request("telegram", "user123", None).unwrap();
    assert!(r1.created);
    let r2 = store
        .upsert_request("telegram", "user123", Some(serde_json::json!({"x": 1})))
        .unwrap();
    assert!(!r2.created);
    assert_eq!(r1.code, r2.code);

    let pending = store.list_pending("telegram").unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "user123");
    assert_eq!(pending[0].meta, Some(serde_json::json!({"x": 1})));
}

#[test]
fn test_approve_adds_to_allow_from() {
    let (store, _) = test_store();
    let r = store.upsert_request("telegram", "user456", None).unwrap();
    assert!(r.created);

    let approved = store.approve("telegram", &r.code).unwrap();
    assert!(approved.is_some());
    assert_eq!(approved.unwrap().id, "user456");

    let allow = store.read_allow_from("telegram").unwrap();
    assert_eq!(allow, vec!["user456"]);
}

#[test]
fn test_approve_case_insensitive_code() {
    let (store, _) = test_store();
    let r = store.upsert_request("telegram", "user789", None).unwrap();
    let code_lower = r.code.to_lowercase();
    let approved = store.approve("telegram", &code_lower).unwrap();
    assert!(approved.is_some());
}

#[test]
fn test_approve_invalid_code_returns_none() {
    let (store, _) = test_store();
    store.upsert_request("telegram", "user123", None).unwrap();
    let approved = store.approve("telegram", "BADCODE1").unwrap();
    assert!(approved.is_none());
}

#[test]
fn test_approve_rate_limited_after_many_failures() {
    let (store, _) = test_store();
    store.upsert_request("telegram", "user123", None).unwrap();
    for _ in 0..PAIRING_APPROVE_RATE_LIMIT {
        let _ = store.approve("telegram", "WRONG01");
    }
    let err = store.approve("telegram", "WRONG02").unwrap_err();
    assert!(matches!(err, PairingStoreError::ApproveRateLimited));
}

#[test]
fn test_is_sender_allowed_by_id() {
    let (store, _) = test_store();
    let r = store.upsert_request("telegram", "user999", None).unwrap();
    store.approve("telegram", &r.code).unwrap();

    assert!(
        store
            .is_sender_allowed("telegram", "user999", None)
            .unwrap()
    );
    assert!(!store.is_sender_allowed("telegram", "other", None).unwrap());
}

#[test]
fn test_is_sender_allowed_by_username() {
    let (store, _) = test_store();
    store
        .upsert_request(
            "telegram",
            "alice",
            Some(serde_json::json!({"username": "alice"})),
        )
        .unwrap();
    let pending = store.list_pending("telegram").unwrap();
    store.approve("telegram", &pending[0].code).unwrap();

    // approve adds id to allow_from. For username we need to add it manually.
    // Actually approve adds entry.id which is "alice". So is_sender_allowed("telegram", "alice", None) would work.
    assert!(store.is_sender_allowed("telegram", "alice", None).unwrap());
    assert!(
        store
            .is_sender_allowed("telegram", "alice", Some("alice"))
            .unwrap()
    );
}

#[test]
fn test_channel_normalization() {
    let (store, _) = test_store();
    store.upsert_request("Telegram", "u1", None).unwrap();
    let pending = store.list_pending("telegram").unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "u1");
}

#[test]
fn test_invalid_channel_rejected() {
    let (store, _) = test_store();
    store.upsert_request("telegram", "u1", None).unwrap();
    store.list_pending("").unwrap_err();
    store.upsert_request("", "u1", None).unwrap_err();
}
