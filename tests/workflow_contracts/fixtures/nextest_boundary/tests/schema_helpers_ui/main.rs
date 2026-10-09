//! Stands in for the real `schema_helpers_ui` compile-contract binary.
//!
//! Declared as `tests/<name>/main.rs` because that is the form the real one
//! takes. Its tests share a marker with `trybuild` to check that nextest keeps
//! the two processes from overlapping.

#[test]
fn shared_preparation_contract_a() {
    nextest_boundary_fixture::detect_test_process_overlap()
        .expect("schema helper fixture overlapped another compile-contract process");
}

#[test]
fn shared_preparation_contract_b() {
    nextest_boundary_fixture::detect_test_process_overlap()
        .expect("schema helper fixture overlapped another compile-contract process");
}

#[test]
fn bounded_sleep() {
    nextest_boundary_fixture::sleep_for_requested_seconds();
}
