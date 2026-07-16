//! Pairing-code generation: random human-friendly codes with collision
//! avoidance against outstanding requests.

use std::collections::HashSet;

use rand::Rng;
use rand::rngs::OsRng;

pub(super) const PAIRING_CODE_LENGTH: usize = 8;
pub(super) const PAIRING_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub(super) fn random_code() -> String {
    let mut rng = OsRng;
    (0..PAIRING_CODE_LENGTH)
        .map(|_| {
            let idx = rng.gen_range(0..PAIRING_ALPHABET.len());
            PAIRING_ALPHABET[idx] as char
        })
        .collect()
}

pub(super) fn generate_unique_code(existing: &HashSet<String>) -> String {
    let mut rng = OsRng;
    for _ in 0..500 {
        let code = random_code();
        if !existing.contains(&code) {
            return code;
        }
    }
    // Fallback: add suffix
    format!("{}{:04}", random_code(), rng.gen_range(0..10000))
}
