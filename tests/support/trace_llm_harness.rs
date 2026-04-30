//! Support modules compiled only for the `trace_llm_tests` harness.

#[path = "trace_json_patch.rs"]
mod trace_json_patch;
#[path = "trace_llm.rs"]
pub mod trace_llm;
#[path = "trace_provider.rs"]
mod trace_provider;
#[path = "trace_types.rs"]
pub mod trace_types;
