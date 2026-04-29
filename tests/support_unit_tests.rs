//! Unit tests for E2E test support modules.
//!
//! These tests live here (instead of inside `support/*.rs`) so they compile
//! and run exactly once, rather than being duplicated across every harness
//! that imports the support modules under test.

#[path = "support/support_unit.rs"]
mod support;

#[path = "support_unit_tests/assertions_tests.rs"]
mod assertions_tests;
#[path = "support_unit_tests/cleanup_tests.rs"]
mod cleanup_tests;
mod property_tests;
mod test_channel_tests;
#[path = "support_unit_tests/trace_llm_contract_tests.rs"]
mod trace_llm_contract_tests;
mod trace_llm_test_fixtures;
mod trace_llm_tests;
mod trace_support_module_tests;
#[path = "support_unit_tests/trace_types_tests.rs"]
mod trace_types_tests;
