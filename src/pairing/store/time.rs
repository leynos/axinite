//! Timestamp helpers and pending-request expiry for the pairing store.

use std::time::{SystemTime, UNIX_EPOCH};

use super::PairingRequest;

/// TTL for pending pairing requests (minutes, not hours — reduces brute-force window).
const PAIRING_PENDING_TTL_SECS: u64 = 15 * 60;

pub(super) fn now_iso() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    #[allow(clippy::cast_possible_wrap)]
    chrono::DateTime::from_timestamp(now.as_secs() as i64, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_else(|| now.as_secs().to_string())
}

pub(super) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(super) fn parse_timestamp(value: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.timestamp() as u64)
        .or_else(|| value.parse::<u64>().ok())
}

pub(super) fn is_expired(req: &PairingRequest, now_secs: u64) -> bool {
    let created = parse_timestamp(&req.created_at).unwrap_or(0);
    now_secs.saturating_sub(created) > PAIRING_PENDING_TTL_SECS
}
