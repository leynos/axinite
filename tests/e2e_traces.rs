//! End-to-end trace tests covering agent behaviors, tools, memory,
//! scheduling, and error paths.

// Keep fixture helpers local to this harness so unrelated integration
// binaries do not compile them through the shared `support` facade.
#[path = "support/fixtures.rs"]
mod fixtures;
#[path = "support/e2e_traces.rs"]
mod support;

const _: &str = fixtures::FIXTURE_ROOT;
const _: std::time::Duration = fixtures::DEFAULT_TIMEOUT;
const _: std::time::Duration = fixtures::LONG_TIMEOUT;
const _: fn(&str, &str) -> String = fixtures::fixture_path;

#[path = "e2e_traces/advanced_traces.rs"]
mod advanced_traces;
#[path = "e2e_traces/attachments.rs"]
mod attachments;
#[path = "e2e_traces/builtin_tool_coverage.rs"]
mod builtin_tool_coverage;
#[path = "e2e_traces/heartbeat.rs"]
mod heartbeat;
#[path = "e2e_traces/metrics.rs"]
mod metrics;
#[cfg(feature = "libsql")]
#[path = "e2e_traces/recorded_trace.rs"]
mod recorded_trace;
#[path = "e2e_traces/routine_cooldown.rs"]
mod routine_cooldown;
#[path = "e2e_traces/routine_cron.rs"]
mod routine_cron;
#[path = "e2e_traces/routine_event.rs"]
mod routine_event;
#[path = "e2e_traces/routine_system_event.rs"]
mod routine_system_event;
#[path = "e2e_traces/safety_layer.rs"]
mod safety_layer;
#[path = "e2e_traces/spot_checks.rs"]
mod spot_checks;
#[path = "e2e_traces/status_events.rs"]
mod status_events;
#[path = "e2e_traces/thread_scheduling.rs"]
mod thread_scheduling;
#[path = "e2e_traces/tool_coverage.rs"]
mod tool_coverage;
#[path = "e2e_traces/trace_error_path.rs"]
mod trace_error_path;
#[path = "e2e_traces/trace_file_tools.rs"]
mod trace_file_tools;
#[path = "e2e_traces/trace_memory.rs"]
mod trace_memory;
#[cfg(feature = "test-helpers")]
#[path = "e2e_traces/wasm_schema_exposure.rs"]
mod wasm_schema_exposure;
#[path = "e2e_traces/worker_coverage.rs"]
mod worker_coverage;
#[path = "e2e_traces/workspace_coverage.rs"]
mod workspace_coverage;
