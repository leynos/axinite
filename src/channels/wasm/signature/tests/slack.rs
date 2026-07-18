//! Unit tests for Slack HMAC-SHA256 webhook signature verification,
//! including timestamp staleness checks.

use crate::channels::wasm::signature::verify_slack_signature;

/// Helper: compute expected Slack signature for a given secret, timestamp, and body.
fn sign_slack_message(signing_secret: &str, timestamp: &str, body: &[u8]) -> String {
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;

    let mut basestring = Vec::new();
    basestring.extend_from_slice(b"v0:");
    basestring.extend_from_slice(timestamp.as_bytes());
    basestring.push(b':');
    basestring.extend_from_slice(body);

    let mut mac = Hmac::<Sha256>::new_from_slice(signing_secret.as_bytes()).unwrap();
    mac.update(&basestring);
    let computed = mac.finalize().into_bytes();
    format!("v0={}", hex::encode(computed))
}

const SLACK_TEST_TS: i64 = 1234567890;

#[test]
fn test_slack_valid_signature_succeeds() {
    let signing_secret = "my-signing-secret";
    let timestamp = "1234567890";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G&team_id=T1DC2JH3J";

    let signature = sign_slack_message(signing_secret, timestamp, body);
    assert!(verify_slack_signature(
        signing_secret,
        timestamp,
        body,
        &signature,
        SLACK_TEST_TS
    ));
}

#[test]
fn test_slack_tampered_body_fails() {
    let signing_secret = "my-signing-secret";
    let timestamp = "1234567890";
    let original_body = b"token=xyzz0WbapA4vBCDEFasx0q6G&team_id=T1DC2JH3J";
    let tampered_body = b"token=MODIFIED&team_id=T1DC2JH3J";

    let signature = sign_slack_message(signing_secret, timestamp, original_body);
    assert!(
        !verify_slack_signature(
            signing_secret,
            timestamp,
            tampered_body,
            &signature,
            SLACK_TEST_TS
        ),
        "Signature for different body should fail"
    );
}

#[test]
fn test_slack_tampered_timestamp_fails() {
    let signing_secret = "my-signing-secret";
    let timestamp = "1234567890";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G&team_id=T1DC2JH3J";

    let signature = sign_slack_message(signing_secret, timestamp, body);
    assert!(
        !verify_slack_signature(
            signing_secret,
            "9999999999", // Different timestamp in signature
            body,
            &signature,
            SLACK_TEST_TS
        ),
        "Signature with wrong timestamp should fail"
    );
}

#[test]
fn test_slack_tampered_signature_fails() {
    let signing_secret = "my-signing-secret";
    let timestamp = "1234567890";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G&team_id=T1DC2JH3J";

    let signature = sign_slack_message(signing_secret, timestamp, body);
    // Flip a byte in the signature hex (change first char after "v0=")
    let chars: Vec<char> = signature.chars().collect();
    let mut new_chars = chars.clone();
    if chars.len() > 3 {
        new_chars[3] = if chars[3] == 'a' { 'b' } else { 'a' };
    }
    let modified_sig: String = new_chars.iter().collect();

    assert!(
        !verify_slack_signature(
            signing_secret,
            timestamp,
            body,
            &modified_sig,
            SLACK_TEST_TS
        ),
        "Tampered signature should fail"
    );
}

#[test]
fn test_slack_stale_timestamp_rejected() {
    let signing_secret = "my-signing-secret";
    let timestamp = "1234567890";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G";

    let signature = sign_slack_message(signing_secret, timestamp, body);
    // now_secs is 400 seconds after timestamp — too stale
    assert!(
        !verify_slack_signature(
            signing_secret,
            timestamp,
            body,
            &signature,
            SLACK_TEST_TS + 400
        ),
        "Stale timestamp (400s old) should be rejected"
    );
}

#[test]
fn test_slack_future_timestamp_rejected() {
    let signing_secret = "my-signing-secret";
    let timestamp = "1234567890";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G";

    let signature = sign_slack_message(signing_secret, timestamp, body);
    // now_secs is 400 seconds before timestamp — future
    assert!(
        !verify_slack_signature(
            signing_secret,
            timestamp,
            body,
            &signature,
            SLACK_TEST_TS - 400
        ),
        "Future timestamp (400s ahead) should be rejected"
    );
}

#[test]
fn test_slack_boundary_300s_accepted() {
    let signing_secret = "my-signing-secret";
    let timestamp = "1234567890";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G";

    let signature = sign_slack_message(signing_secret, timestamp, body);
    // Exactly 300 seconds difference — should be accepted
    assert!(
        verify_slack_signature(
            signing_secret,
            timestamp,
            body,
            &signature,
            SLACK_TEST_TS + 300
        ),
        "Timestamp exactly 300s old should be accepted"
    );
}

#[test]
fn test_slack_boundary_301s_rejected() {
    let signing_secret = "my-signing-secret";
    let timestamp = "1234567890";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G";

    let signature = sign_slack_message(signing_secret, timestamp, body);
    // 301 seconds difference — should be rejected
    assert!(
        !verify_slack_signature(
            signing_secret,
            timestamp,
            body,
            &signature,
            SLACK_TEST_TS + 301
        ),
        "Timestamp 301s old should be rejected"
    );
}

#[test]
fn test_slack_non_numeric_timestamp_rejected() {
    let signing_secret = "my-signing-secret";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G";

    assert!(
        !verify_slack_signature(signing_secret, "not-a-number", body, "v0=abc123", 0),
        "Non-numeric timestamp should be rejected"
    );
}

#[test]
fn test_slack_missing_v0_prefix_fails() {
    let signing_secret = "my-signing-secret";
    let timestamp = "1234567890";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G";

    let signature = sign_slack_message(signing_secret, timestamp, body);
    // Remove the "v0=" prefix
    let bad_sig = signature.strip_prefix("v0=").unwrap_or(&signature);

    assert!(
        !verify_slack_signature(signing_secret, timestamp, body, bad_sig, SLACK_TEST_TS),
        "Missing v0= prefix should fail"
    );
}

#[test]
fn test_slack_wrong_signing_secret_fails() {
    let secret_a = "secret-a";
    let secret_b = "secret-b";
    let timestamp = "1234567890";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G";

    let signature = sign_slack_message(secret_a, timestamp, body);
    // Try to verify with a different secret
    assert!(
        !verify_slack_signature(secret_b, timestamp, body, &signature, SLACK_TEST_TS),
        "Signature from different secret should fail"
    );
}

#[test]
fn test_slack_empty_body_valid() {
    let signing_secret = "my-signing-secret";
    let timestamp = "1234567890";
    let body = b"";

    let signature = sign_slack_message(signing_secret, timestamp, body);
    assert!(
        verify_slack_signature(signing_secret, timestamp, body, &signature, SLACK_TEST_TS),
        "Empty body with valid signature should succeed"
    );
}

#[test]
fn test_slack_negative_timestamp_rejected() {
    let signing_secret = "my-signing-secret";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G";

    assert!(
        !verify_slack_signature(signing_secret, "-1", body, "v0=abc123", 0),
        "Negative timestamp should be rejected"
    );
}

#[test]
fn test_slack_empty_timestamp_rejected() {
    let signing_secret = "my-signing-secret";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G";

    assert!(
        !verify_slack_signature(signing_secret, "", body, "v0=abc123", 0),
        "Empty timestamp should be rejected"
    );
}
