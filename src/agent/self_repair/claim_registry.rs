//! Mutex-protected claim lifecycle shared by Axinite and its proof package.

use std::collections::HashSet;
use std::fmt;
use std::sync::Mutex;

/// Acquisition or release could not lock the poisoned registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClaimPoisoned;

impl fmt::Display for ClaimPoisoned {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("poisoned lock: another task failed inside")
    }
}

impl std::error::Error for ClaimPoisoned {}

/// Owns the single synchronization boundary for insertion and removal.
#[derive(Default)]
pub(super) struct ClaimRegistry {
    active: Mutex<HashSet<String>>,
}

impl ClaimRegistry {
    /// Acquire an unclaimed name; the observer only reports the release result.
    pub(super) fn claim(
        &self,
        name: &str,
        on_release: fn(&str, Result<(), ClaimPoisoned>),
    ) -> Result<Option<RegistryClaim<'_>>, ClaimPoisoned> {
        let mut active = self.active.lock().map_err(|_| ClaimPoisoned)?;
        if !active.insert(name.to_owned()) {
            return Ok(None);
        }
        Ok(Some(RegistryClaim {
            registry: self,
            name: name.to_owned(),
            on_release,
        }))
    }
}

/// Releases its own name on drop, unless the registry mutex is poisoned.
pub(super) struct RegistryClaim<'a> {
    registry: &'a ClaimRegistry,
    name: String,
    on_release: fn(&str, Result<(), ClaimPoisoned>),
}

impl Drop for RegistryClaim<'_> {
    fn drop(&mut self) {
        match self.registry.active.lock() {
            Ok(mut active) => {
                active.remove(&self.name);
                // Preserve the production telemetry's lock lifetime, including
                // poisoning if an observer panics after the release transition.
                (self.on_release)(&self.name, Ok(()));
            }
            Err(_held_poisoned_lock) => (self.on_release)(&self.name, Err(ClaimPoisoned)),
        }
    }
}

#[cfg(kani)]
#[path = "claim_registry_verification.rs"]
mod verification;

#[cfg(test)]
pub(super) mod tests {
    //! Executable poisoning tests, intentionally outside the Kani proof scope.

    use super::{ClaimPoisoned, ClaimRegistry};

    pub(crate) fn poison(registry: &ClaimRegistry) -> std::thread::Result<()> {
        std::panic::catch_unwind(|| {
            if let Ok(_held_lock) = registry.active.lock() {
                panic!("deliberately poison the registry while its mutex is held");
            }
        })
    }

    #[test]
    fn poisoned_acquisition_fails() {
        let registry = ClaimRegistry::default();
        poison(&registry).expect_err("the lock holder must panic");
        assert!(matches!(registry.claim("a", |_, _| {}), Err(ClaimPoisoned)));
    }

    #[test]
    fn poisoned_drop_reports_failure_and_retains_entry() {
        let registry = ClaimRegistry::default();
        let guard = registry
            .claim("a", |name, result| {
                assert_eq!(name, "a");
                assert_eq!(result, Err(ClaimPoisoned));
            })
            .expect("initial lock is healthy")
            .expect("initial name is unclaimed");
        poison(&registry).expect_err("the lock holder must panic");
        drop(guard);
        let error = registry
            .active
            .lock()
            .expect_err("drop must not heal the lock");
        // Inspect the retained entry without changing the production policy.
        assert!(error.get_ref().contains("a"));
    }

    #[test]
    fn panicking_observer_preserves_release_and_poisoning_order() {
        let registry = ClaimRegistry::default();
        let guard = registry
            .claim("a", |_, _| {
                panic!("simulate a panicking telemetry observer")
            })
            .expect("healthy mutex")
            .expect("unclaimed name");
        std::panic::catch_unwind(|| drop(guard)).expect_err("observer must panic");
        let error = registry.active.lock().expect_err("observer held the mutex");
        assert!(error.get_ref().is_empty(), "removal precedes observation");
    }

    #[test]
    fn release_observer_sees_success_and_other_claim_survives() {
        let registry = ClaimRegistry::default();
        let first = registry
            .claim("a", |name, result| {
                assert_eq!(name, "a");
                assert_eq!(result, Ok(()));
            })
            .expect("healthy mutex")
            .expect("unclaimed name");
        let second = registry
            .claim("b", |_, _| {})
            .expect("healthy mutex")
            .expect("distinct name");
        drop(first);
        {
            let active = registry.active.lock().expect("healthy mutex after release");
            assert!(!active.contains("a"));
            assert!(active.contains("b"));
            assert_eq!(active.len(), 1);
        }
        drop(second);
        assert!(registry.active.lock().expect("healthy mutex").is_empty());
    }
}
