//! Error handling and edge case tests for the OpenClaw importer.
//!
//! Split by theme: `config_errors`, `database_errors`, `edge_cases`,
//! and `workspace_errors`.

#[path = "config_errors.rs"]
mod config_errors;
#[path = "database_errors.rs"]
mod database_errors;
#[path = "edge_cases.rs"]
mod edge_cases;
#[path = "workspace_errors.rs"]
mod workspace_errors;
