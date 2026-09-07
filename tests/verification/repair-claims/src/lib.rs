//! Independent proof package compiling the production registry source.

#![forbid(unsafe_code)]

#[cfg(any(test, kani))]
#[path = "../../../../src/agent/self_repair/claim_registry.rs"]
mod claim_registry;
