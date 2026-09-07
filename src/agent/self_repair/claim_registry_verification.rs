//! Bounded sequential proofs; no real thread interleavings are explored.

use super::{ClaimRegistry, RegistryClaim};

fn claim<'a>(registry: &'a ClaimRegistry, name: &str) -> RegistryClaim<'a> {
    match registry.claim(name, |_, _| {}) {
        Ok(Some(guard)) => guard,
        _ => panic!("an unclaimed name must be claimable"),
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn dropped_claim_can_be_reclaimed() {
    let registry = ClaimRegistry::default();
    let guard = claim(&registry, "a");
    drop(guard);
    let reclaimed = claim(&registry, "a");
    drop(reclaimed);
    assert!(registry.active.lock().unwrap().is_empty());
}
