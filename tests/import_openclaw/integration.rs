//! Integration tests for OpenClaw import with actual database state verification.
//!
//! These tests exercise the full import pipeline with real database writes,
//! verifying that data is correctly stored, idempotent, and that dry-run mode
//! prevents modifications.

#![cfg(all(feature = "import", feature = "libsql"))]

#[cfg(all(feature = "import", feature = "libsql"))]
#[path = "integration/import_integration_tests.rs"]
mod import_integration_tests;
