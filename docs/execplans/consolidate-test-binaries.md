# Consolidate Integration Test Binaries

**Branch:** (to be created from `build-time`)
**Date:** 2026-03-15
**Status:** Plan ready; not yet started
**Estimated impact:** 3–4 min saved on incremental `make test`

## Big Picture

Reduce the number of integration test binaries from 43 to ~8–10 by
grouping related test files into module trees under fewer top-level
harnesses. Each top-level `.rs` file in `tests/` compiles as a separate
binary, linked against the full ironclaw crate and all dev-dependencies.
With 43 binaries, a single source change triggers 43 relink operations
(measured at 6 min 05 s incremental). Consolidation targets ~8–10 binaries,
cutting link time roughly in proportion.

## Constraints

- Test coverage must not decrease.
- Test isolation must not regress — tests that currently run in parallel via
  nextest must continue to do so (nextest parallelizes at the test-function
  level, not the binary level, so merging binaries is safe).
- The shared `tests/support/` module must remain importable by all harnesses
  that need it.
- `tests/html_to_markdown.rs` must remain a separate `[[test]]` binary
  because it has `required-features = ["html-to-markdown"]` in
  `Cargo.toml`.
- Feature-gated tests (e.g., `#[cfg(feature = "libsql")]`) must retain
  their gates inside the merged modules.

## Current Test File Inventory (40 files)


### End-to-end (E2E) Trace Tests (15 files, all use `mod support;`)

- `e2e_advanced_traces.rs`
- `e2e_attachments.rs`
- `e2e_builtin_tool_coverage.rs`
- `e2e_metrics_test.rs`
- `e2e_recorded_trace.rs`
- `e2e_safety_layer.rs`
- `e2e_spot_checks.rs`
- `e2e_status_events.rs`
- `e2e_thread_scheduling.rs`
- `e2e_tool_coverage.rs`
- `e2e_trace_error_path.rs`
- `e2e_trace_file_tools.rs`
- `e2e_trace_memory.rs`
- `e2e_worker_coverage.rs`
- `e2e_workspace_coverage.rs`

### End-to-end (E2E) Trace Tests (15 files, all use `mod support;`)

- `e2e_advanced_traces.rs`
- `e2e_attachments.rs`
- `e2e_builtin_tool_coverage.rs`
- `e2e_metrics_test.rs`
- `e2e_recorded_trace.rs`
- `e2e_safety_layer.rs`
- `e2e_spot_checks.rs`
- `e2e_status_events.rs`
- `e2e_thread_scheduling.rs`
- `e2e_tool_coverage.rs`
- `e2e_trace_error_path.rs`
- `e2e_trace_file_tools.rs`
- `e2e_trace_memory.rs`
- `e2e_worker_coverage.rs`
- `e2e_workspace_coverage.rs`

### Import Tests (6 files, standalone)

- `import_openclaw.rs`
- `import_openclaw_comprehensive.rs`
- `import_openclaw_e2e.rs`
- `import_openclaw_errors.rs`
- `import_openclaw_idempotency.rs`
- `import_openclaw_integration.rs`

### Channel/Network Tests (5 files, 1 uses `mod support;`)

- `openai_compat_integration.rs`
- `relay_integration.rs`
- `telegram_auth_integration.rs`
- `wasm_channel_integration.rs`
- `ws_gateway_integration.rs`

### Integration/Misc Tests (5 files, standalone)

- `heartbeat_integration.rs`
- `pairing_integration.rs`
- `provider_chaos.rs`
- `sighup_reload_integration.rs`
- `workspace_integration.rs`

### Config/Data/Tool Tests (4 files)

- `config_round_trip.rs`
- `trace_format.rs`
- `tool_schema_validation.rs`
- `wit_compat.rs`


### Other (5 files)

- `html_to_markdown.rs` (required-features gated, must stay separate)
- `trace_llm_tests.rs`
- `support_unit_tests.rs`
- `libsql_wit_defaults_integration.rs`
- `module_init_integration.rs`

### Other (5 files)

- `html_to_markdown.rs` (required-features gated, must stay separate)
- `trace_llm_tests.rs`
- `support_unit_tests.rs`
- `libsql_wit_defaults_integration.rs`
- `module_init_integration.rs`

## Target Structure (10 binaries)

```
tests/
├── support/                          # Shared module (unchanged)
│   ├── mod.rs
│   ├── assertions.rs
│   ├── cleanup.rs
│   ├── instrumented_llm.rs
│   ├── metrics.rs
│   ├── telegram.rs
│   ├── test_channel.rs
│   ├── test_rig.rs
│   └── trace_llm.rs
├── e2e_traces.rs                     # Harness: 15 e2e trace modules
│   └── e2e_traces/
│       ├── advanced_traces.rs
│       ├── attachments.rs
│       ├── builtin_tool_coverage.rs
│       ├── metrics.rs
│       ├── recorded_trace.rs
│       ├── safety_layer.rs
│       ├── spot_checks.rs
│       ├── status_events.rs
│       ├── thread_scheduling.rs
│       ├── tool_coverage.rs
│       ├── trace_error_path.rs
│       ├── trace_file_tools.rs
│       ├── trace_memory.rs
│       ├── worker_coverage.rs
│       └── workspace_coverage.rs
├── import_openclaw.rs                # Harness: 6 import modules
│   └── import_openclaw/
│       ├── basic.rs
│       ├── comprehensive.rs
│       ├── e2e.rs
│       ├── errors.rs
│       ├── idempotency.rs
│       └── integration.rs
├── channels.rs                       # Harness: 5 channel tests
│   └── channels/
│       ├── openai_compat.rs
│       ├── relay.rs
│       ├── telegram_auth.rs
│       ├── wasm_channel.rs
│       └── ws_gateway.rs
├── infrastructure.rs                 # Harness: 5 misc integration tests
│   └── infrastructure/
│       ├── heartbeat.rs
│       ├── pairing.rs
│       ├── provider_chaos.rs
│       ├── sighup_reload.rs
│       └── workspace.rs
├── tools_and_config.rs               # Harness: 4 tool/config tests
│   └── tools_and_config/
│       ├── config_round_trip.rs
│       ├── trace_format.rs
│       ├── tool_schema_validation.rs
│       └── wit_compat.rs
├── db_integration.rs                 # Harness: 2 database tests
│   └── db_integration/
│       ├── libsql_wit_defaults.rs
│       └── module_init.rs
├── support_unit_tests.rs             # Keep separate (tests support module)
├── trace_llm_tests.rs                # Keep separate (tests support module)
├── html_to_markdown.rs               # Keep separate (required-features)
└── e2e/                              # Python tests (unchanged)
```

**Result: 10 binaries** (down from 43).

## Implementation Steps

### Phase 1: Create harness structure

- [ ] Create subdirectory for each harness group (e.g.,
  `tests/e2e_traces/`)
- [ ] For each group, create the top-level harness file (e.g.,
  `tests/e2e_traces.rs`) containing `mod support;` (if needed) and `mod`
  declarations for each submodule
- [ ] Move existing test files into the subdirectories, renaming as needed
- [ ] Adjust `mod support;` imports — in the new structure, only the
  top-level harness needs `mod support;`; submodules access it via
  `crate::support::*` or `super::support::*`

### Phase 2: Fix module paths

- [ ] Update any `use crate::*` or `use super::*` imports in moved test
  files
- [ ] Ensure `#[cfg(feature = "...")]` gates are preserved on individual
  test functions or modules
- [ ] Verify that `tests/support/` is still reachable from all harnesses
  that need it

### Phase 3: Update Cargo.toml

- [ ] Remove the existing `[[test]]` entry for `html_to_markdown` only if
  its path changes (it should not)
- [ ] Verify no other `[[test]]` entries are needed for the new structure
  (Cargo auto-discovers `tests/*.rs`)

### Phase 4: Validate

- [ ] Run `cargo nextest run --workspace --features test-helpers` — all
  3,209 tests must pass
- [ ] Run `cargo test --manifest-path tools-src/github/Cargo.toml`
- [ ] Verify test count matches pre-consolidation count
- [ ] Time `make test` with a one-file touch to confirm link-time
  improvement

## Risks

- **Test name collisions:** If two files in different groups define a test
  with the same name, they will collide within the same binary. Mitigated by
  using `mod` scoping (each submodule is its own namespace).
- **Shared mutable state:** Tests that mutate global state (e.g.,
  environment variables) may interfere when colocated in the same binary.
  nextest runs each test in its own process by default, so this is mitigated
  at the runner level.
- **`mod support;` path resolution:** The `support/` directory must be a
  sibling of the harness `.rs` file. Since all harnesses are at
  `tests/*.rs`, the existing `tests/support/` works unchanged.

## Progress

- [ ] Phase 1: Create harness structure
- [ ] Phase 2: Fix module paths
- [ ] Phase 3: Update Cargo.toml
- [ ] Phase 4: Validate
