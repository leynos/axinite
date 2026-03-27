//! Unit tests for E2E test support modules.
//!
//! These tests live here (instead of inside `support/*.rs`) so they compile
//! and run exactly once, rather than being duplicated across every `e2e_*.rs`
//! test binary that declares `mod support;`.

mod support;

#[path = "support_unit_tests/assertions_tests.rs"]
mod assertions_tests;
#[path = "support_unit_tests/cleanup_tests.rs"]
mod cleanup_tests;
#[path = "support_unit_tests/test_channel_tests.rs"]
mod test_channel_tests;
#[cfg(feature = "libsql")]
#[path = "support_unit_tests/test_rig_tests.rs"]
mod test_rig_tests;
#[path = "support_unit_tests/trace_llm_contract_tests.rs"]
mod trace_llm_contract_tests;
#[path = "support_unit_tests/trace_llm_tests.rs"]
mod trace_llm_tests;
