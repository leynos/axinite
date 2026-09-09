//! Minimal standard-library compatibility probe, not a lifecycle proof.
#[kani::proof]
#[kani::unwind(2)]
fn default_hashset() {
    let names = std::collections::HashSet::<String>::new();
    assert!(names.is_empty());
}
