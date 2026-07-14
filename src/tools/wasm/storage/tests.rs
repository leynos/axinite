//! Unit tests for WASM binary hashing, integrity, and trust levels.

use crate::tools::wasm::storage::{
    ToolStatus, TrustLevel, compute_binary_hash, verify_binary_integrity,
};

#[test]
fn test_compute_hash() {
    let binary = b"(module)";
    let hash = compute_binary_hash(binary);
    assert_eq!(hash.len(), 32); // BLAKE3 produces 32-byte hash
}

#[test]
fn test_verify_integrity_success() {
    let binary = b"test wasm binary content";
    let hash = compute_binary_hash(binary);
    assert!(verify_binary_integrity(binary, &hash));
}

#[test]
fn test_verify_integrity_failure() {
    let binary = b"test wasm binary content";
    let hash = compute_binary_hash(binary);
    let tampered = b"tampered wasm binary content";
    assert!(!verify_binary_integrity(tampered, &hash));
}

#[test]
fn test_trust_level_parse() {
    assert_eq!("system".parse::<TrustLevel>().unwrap(), TrustLevel::System);
    assert_eq!(
        "verified".parse::<TrustLevel>().unwrap(),
        TrustLevel::Verified
    );
    assert_eq!("user".parse::<TrustLevel>().unwrap(), TrustLevel::User);
    assert!("invalid".parse::<TrustLevel>().is_err());
}

#[test]
fn test_status_parse() {
    assert_eq!("active".parse::<ToolStatus>().unwrap(), ToolStatus::Active);
    assert_eq!(
        "disabled".parse::<ToolStatus>().unwrap(),
        ToolStatus::Disabled
    );
    assert_eq!(
        "quarantined".parse::<ToolStatus>().unwrap(),
        ToolStatus::Quarantined
    );
    assert!("invalid".parse::<ToolStatus>().is_err());
}
