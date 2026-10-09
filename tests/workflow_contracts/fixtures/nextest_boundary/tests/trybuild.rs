//! Stands in for the real `trybuild` compile-contract binary.
//!
//! Multiple tests exercise the same shared preparation group. Each writes the
//! marker also used by `schema_helpers_ui`, so removing serialization makes
//! this binary race the other compile-contract target.

#[test]
fn shared_preparation_contract_a() {
    nextest_boundary_fixture::detect_test_process_overlap()
        .expect("trybuild fixture overlapped another compile-contract process");
}

#[test]
fn shared_preparation_contract_b() {
    nextest_boundary_fixture::detect_test_process_overlap()
        .expect("trybuild fixture overlapped another compile-contract process");
}

#[test]
fn bounded_sleep() {
    nextest_boundary_fixture::sleep_for_requested_seconds();
}
