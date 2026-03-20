//! Tests for configuration round-trip, trace format, tool schema validation,
//! and WIT compatibility.

mod support;

#[path = "tools_and_config/config_round_trip.rs"]
mod config_round_trip;
#[path = "tools_and_config/tool_schema_validation.rs"]
mod tool_schema_validation;
#[path = "tools_and_config/trace_format.rs"]
mod trace_format;
#[path = "tools_and_config/wit_compat.rs"]
mod wit_compat;
