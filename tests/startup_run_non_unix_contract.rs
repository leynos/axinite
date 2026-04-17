#![cfg(not(unix))]

//! Non-Unix startup compile contract wired into `cargo check --tests`.
//!
//! The dedicated Windows CI job compiles this integration test even though it
//! does not execute the trybuild suite, so the non-Unix startup surface still
//! receives target-appropriate type checking.

#[path = "trybuild/startup_run_non_unix.rs"]
mod startup_run_non_unix_contract;

#[test]
fn startup_run_non_unix_contract_compiles() {}
