//! Independent proof package compiling the production registry source.

#![forbid(unsafe_code)]

#[cfg(any(test, kani))]
#[path = "../../../../src/agent/self_repair/claim_registry.rs"]
mod claim_registry;

/// Standard-library compatibility probe for the released verifier.
///
/// This is not a lifecycle proof. It constructs the default `HashSet` that the
/// production registry uses, so a verifier that cannot model the default
/// `RandomState` entropy path fails here, before any lifecycle harness runs.
#[cfg(kani)]
mod compatibility {
    #[kani::proof]
    #[kani::unwind(2)]
    fn default_hashset_is_constructible() {
        let names = std::collections::HashSet::<String>::new();
        assert!(names.is_empty());
    }
}
