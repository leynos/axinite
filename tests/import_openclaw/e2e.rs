//! End-to-end integration tests for OpenClaw importer with actual import execution.
//!
//! These tests verify the complete import pipeline: configuration, settings,
//! credentials, memory chunks, workspace documents, and conversations.
//!
//! - [`harness`] — synthetic OpenClaw environment builders
//! - [`config`] — configuration, settings, and credential extraction tests
//! - [`data`] — data volume, stats, error handling, and extensibility tests

#![cfg(feature = "import")]

// `#[path]` is required: this module is itself declared with a `#[path]`
// attribute from `tests/import_openclaw.rs`, so child modules would otherwise
// resolve relative to `tests/import_openclaw/` rather than this directory.
#[path = "e2e/config.rs"]
mod config;
#[path = "e2e/data.rs"]
mod data;
#[path = "e2e/harness.rs"]
mod harness;
