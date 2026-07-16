//! Webhook signature verification (Discord Ed25519 and Slack HMAC-SHA256).
//!
//! Validates request signatures for incoming webhooks:
//! - Discord: `X-Signature-Ed25519` and `X-Signature-Timestamp` headers
//! - Slack: `X-Slack-Signature` and `X-Slack-Request-Timestamp` headers
//!
//! See: <https://discord.com/developers/docs/interactions/overview#validating-security-request-headers>
//! See: <https://api.slack.com/authentication/verifying-requests-from-slack>

/// Verify a Discord interaction signature.
///
/// Discord signs each interaction with Ed25519 using:
/// - message = `timestamp` (UTF-8 bytes) ++ `body` (raw bytes)
/// - signature = Ed25519 detached signature (hex-encoded in header)
/// - public_key = Application public key from Developer Portal (hex-encoded)
///
/// Returns `true` if the signature is valid, `false` on any error
/// (bad hex, wrong length, invalid signature, etc.).
pub fn verify_discord_signature(
    public_key_hex: &str,
    signature_hex: &str,
    timestamp: &str,
    body: &[u8],
    now_secs: i64,
) -> bool {
    // Staleness check: reject non-numeric or stale/future timestamps
    let ts: i64 = match timestamp.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if (now_secs - ts).abs() > 5 {
        return false;
    }
    use ed25519_dalek::{Signature, VerifyingKey};

    let Ok(sig_bytes) = hex::decode(signature_hex) else {
        return false;
    };
    let Ok(key_bytes) = hex::decode(public_key_hex) else {
        return false;
    };
    let Ok(signature) = Signature::from_slice(&sig_bytes) else {
        return false;
    };
    let Ok(verifying_key) = VerifyingKey::try_from(key_bytes.as_slice()) else {
        return false;
    };

    let mut message = Vec::with_capacity(timestamp.len() + body.len());
    message.extend_from_slice(timestamp.as_bytes());
    message.extend_from_slice(body);
    verifying_key.verify_strict(&message, &signature).is_ok()
}

/// Verify a Slack webhook signature using HMAC-SHA256.
///
/// Slack signs each webhook request with HMAC-SHA256 using:
/// - basestring = `"v0:" + timestamp + ":" + body`
/// - signature = hex-encoded HMAC-SHA256(signing_secret, basestring)
/// - header = `"v0=" + signature` (in `X-Slack-Signature` header)
///
/// Includes staleness check: rejects requests with timestamps older than 5 minutes.
/// Returns `true` if the signature is valid, `false` on any error
/// (bad timing, mismatched signature, invalid format, etc.).
pub fn verify_slack_signature(
    signing_secret: &str,
    timestamp: &str,
    body: &[u8],
    signature_header: &str,
    now_secs: i64,
) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    // 1. Parse and check staleness (5-minute window)
    let ts: i64 = match timestamp.parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if (now_secs - ts).abs() > 300 {
        return false;
    }

    // 2. Build the basestring: "v0:{timestamp}:{body}"
    let mut basestring = Vec::with_capacity(3 + timestamp.len() + 1 + body.len());
    basestring.extend_from_slice(b"v0:");
    basestring.extend_from_slice(timestamp.as_bytes());
    basestring.push(b':');
    basestring.extend_from_slice(body);

    // 3. Compute HMAC-SHA256
    let mut mac = match Hmac::<Sha256>::new_from_slice(signing_secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(&basestring);
    let computed = mac.finalize().into_bytes();
    let computed_hex = hex::encode(computed);
    let expected = format!("v0={}", computed_hex);

    // 4. Constant-time compare (avoids timing side-channels)
    use subtle::ConstantTimeEq;
    expected
        .as_bytes()
        .ct_eq(signature_header.as_bytes())
        .into()
}

#[cfg(test)]
mod tests;
