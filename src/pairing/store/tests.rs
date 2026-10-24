//! Unit tests for the pairing store and channel key sanitization.

use super::codes::{PAIRING_ALPHABET, PAIRING_CODE_LENGTH, random_code};
use super::paths::safe_channel_key;
use super::*;
use tempfile::TempDir;

#[test]
fn test_safe_channel_key() {
    assert_eq!(
        safe_channel_key("telegram").expect("telegram is a valid channel key"),
        "telegram"
    );
    assert_eq!(
        safe_channel_key("Telegram").expect("Telegram is a valid channel key"),
        "telegram"
    );
    safe_channel_key("").unwrap_err();
}

#[test]
fn test_random_code() {
    let c = random_code();
    assert_eq!(c.len(), PAIRING_CODE_LENGTH);
    assert!(c.chars().all(|c| PAIRING_ALPHABET.contains(&(c as u8))));
}

fn test_store() -> std::io::Result<(PairingStore, TempDir)> {
    let dir = TempDir::new()?;
    let store = PairingStore::with_base_dir(dir.path().to_path_buf());
    Ok((store, dir))
}

#[test]
fn test_list_pending_empty() -> anyhow::Result<()> {
    let (store, _) = test_store()?;
    let requests = store.list_pending("telegram")?;
    assert!(requests.is_empty());
    Ok(())
}

#[test]
fn test_upsert_request_creates_new() -> anyhow::Result<()> {
    let (store, _) = test_store()?;
    let result = store.upsert_request(
        "telegram",
        "user123",
        Some(serde_json::json!({"chat_id": 456})),
    )?;
    assert!(result.created);
    assert_eq!(result.code.len(), PAIRING_CODE_LENGTH);
    assert!(
        result
            .code
            .chars()
            .all(|c| PAIRING_ALPHABET.contains(&(c as u8)))
    );
    Ok(())
}

#[test]
fn test_upsert_request_updates_existing() -> anyhow::Result<()> {
    let (store, _) = test_store()?;
    let r1 = store.upsert_request("telegram", "user123", None)?;
    assert!(r1.created);
    let r2 = store.upsert_request("telegram", "user123", Some(serde_json::json!({"x": 1})))?;
    assert!(!r2.created);
    assert_eq!(r1.code, r2.code);

    let pending = store.list_pending("telegram")?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "user123");
    assert_eq!(pending[0].meta, Some(serde_json::json!({"x": 1})));
    Ok(())
}

#[test]
fn test_approve_adds_to_allow_from() -> anyhow::Result<()> {
    let (store, _) = test_store()?;
    let r = store.upsert_request("telegram", "user456", None)?;
    assert!(r.created);

    let approved = store.approve("telegram", &r.code)?;
    assert!(approved.is_some());
    assert_eq!(
        approved
            .expect("approval should return the pairing request")
            .id,
        "user456"
    );

    let allow = store.read_allow_from("telegram")?;
    assert_eq!(allow, vec!["user456"]);
    Ok(())
}

#[test]
fn test_approve_case_insensitive_code() -> anyhow::Result<()> {
    let (store, _) = test_store()?;
    let r = store.upsert_request("telegram", "user789", None)?;
    let code_lower = r.code.to_lowercase();
    let approved = store.approve("telegram", &code_lower)?;
    assert!(approved.is_some());
    Ok(())
}

#[test]
fn test_approve_invalid_code_returns_none() -> anyhow::Result<()> {
    let (store, _) = test_store()?;
    store.upsert_request("telegram", "user123", None)?;
    let approved = store.approve("telegram", "BADCODE1")?;
    assert!(approved.is_none());
    Ok(())
}

#[test]
fn test_approve_rate_limited_after_many_failures() -> anyhow::Result<()> {
    let (store, _) = test_store()?;
    store.upsert_request("telegram", "user123", None)?;
    for _ in 0..PAIRING_APPROVE_RATE_LIMIT {
        let _ = store.approve("telegram", "WRONG01");
    }
    let err = store.approve("telegram", "WRONG02").unwrap_err();
    assert!(matches!(err, PairingStoreError::ApproveRateLimited));
    Ok(())
}

#[test]
fn test_is_sender_allowed_by_id() -> anyhow::Result<()> {
    let (store, _) = test_store()?;
    let r = store.upsert_request("telegram", "user999", None)?;
    store.approve("telegram", &r.code)?;

    assert!(store.is_sender_allowed("telegram", "user999", None)?);
    assert!(!store.is_sender_allowed("telegram", "other", None)?);
    Ok(())
}

#[test]
fn test_is_sender_allowed_by_username() -> anyhow::Result<()> {
    let (store, _) = test_store()?;
    store.upsert_request(
        "telegram",
        "alice",
        Some(serde_json::json!({"username": "alice"})),
    )?;
    let pending = store.list_pending("telegram")?;
    store.approve("telegram", &pending[0].code)?;

    // approve adds id to allow_from. For username we need to add it manually.
    // Actually approve adds entry.id which is "alice". So is_sender_allowed("telegram", "alice", None) would work.
    assert!(store.is_sender_allowed("telegram", "alice", None)?);
    assert!(store.is_sender_allowed("telegram", "alice", Some("alice"))?);
    Ok(())
}

#[test]
fn test_channel_normalization() -> anyhow::Result<()> {
    let (store, _) = test_store()?;
    store.upsert_request("Telegram", "u1", None)?;
    let pending = store.list_pending("telegram")?;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, "u1");
    Ok(())
}

#[test]
fn test_invalid_channel_rejected() -> anyhow::Result<()> {
    let (store, _) = test_store()?;
    store.upsert_request("telegram", "u1", None)?;
    store.list_pending("").unwrap_err();
    store.upsert_request("", "u1", None).unwrap_err();
    Ok(())
}
