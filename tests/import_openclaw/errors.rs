//! Error handling and edge case tests for OpenClaw import.
//!
//! These tests verify proper error handling for:
//! - Missing/corrupt files
//! - Invalid configurations
//! - Database corruption
//! - Permission issues
//! - Edge cases in data

#![cfg(feature = "import")]

#[cfg(feature = "import")]
#[path = "errors/error_handling_tests.rs"]
mod error_handling_tests;
