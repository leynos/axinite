//! Tests for the builder core domain types and result structures.
//!
//! These tests cover builder-specific serialization, command planning, and
//! result-shape invariants without invoking the full LLM-driven build loop.

mod assertions;
mod commands;
mod domain_serde;
mod results;
