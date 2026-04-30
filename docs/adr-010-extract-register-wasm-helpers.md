<!-- markdownlint-disable-next-line MD013 -->
# Architectural decision record (ADR) 010: Extract `register_wasm` helpers to reduce cyclomatic complexity

## Status

Accepted.

## Date

2026-04-28

## Context and problem statement

`ToolRegistry::register_wasm` needs to do more than register a wrapper.
It has to prepare the WebAssembly (WASM) component, recover guest metadata when
explicit overrides are missing, apply runtime-scoped overrides, and
persist credential mappings only after registration succeeds.

Keeping that flow in one method makes the orchestration harder to
review and pushes the method towards the repository's cyclomatic
complexity threshold of 9. The WASM registration path also has two
different responsibilities:

- preparation, which compiles the component and assembles the wrapper;
- registration, which inserts the prepared wrapper and performs any
  post-registration persistence.

The codebase therefore benefits from a small helper split rather than a
single large registration method.

The refactor is implemented in
`src/tools/registry/wasm_preparation.rs`, with tests in
`src/tools/registry/wasm_preparation_tests.rs`, and is summarized for
maintainers in
[developer guide section 29](developers-guide.md#29-wasm-tool-schema-normalization).

## Decision drivers

- Keep `register_wasm` readable and reviewable.
- Keep the main registration path below the cyclomatic complexity
  threshold of 9.
- Make metadata recovery and override application explicit.
- Preserve the rule that credential mappings are persisted only after a
  successful registration.
- Keep storage-backed registration as a thin wrapper over the same core
  flow.

## Options considered

- Keep `register_wasm` monolithic.
  - This preserves a single entry point, but it concentrates loading,
    metadata recovery, override application, and persistence logic in
    one method.
- Extract the preparation work into helpers.
  - This separates concerns, keeps the main registration path small, and
    gives each step a stable name.
- Split registration across several unrelated modules.
  - This would reduce the size of the immediate method, but it would
    scatter a single coherent workflow across the tree and make the call
    order harder to follow.

## Decision outcome / proposed direction

Choose the helper extraction.

The registration flow is split as follows:

- `prepare_wasm_tool`
  - accepts `WasmToolRegistration`;
  - compiles the component;
  - builds `PreparedWasmTool`;
  - assembles `WasmMetadataHints` and `WasmRuntimeConfig`;
  - gathers credential mappings;
  - calls `recover_guest_metadata`; and
  - calls `apply_wasm_overrides`.
- `credential_mappings_from_capabilities`
  - extracts HTTP credential mappings from `Capabilities`; and
  - returns them separately so secret material is not read during
    preparation and persistence can wait until insertion succeeds.
- `recover_guest_metadata`
  - asks the compiled wrapper for exported metadata when description or
    schema was not provided explicitly; and
  - preserves explicit overrides over guest-exported values.
- `apply_wasm_overrides`
  - applies explicit description and schema overrides;
  - attaches the secrets store; and
  - attaches OAuth refresh configuration.
- `persist_credential_mappings`
  - stores HTTP credential mappings after a successful registration.

`register_wasm` remains the orchestration entry point. It delegates
preparation, registers the prepared wrapper, and only then persists any
credential mappings. `register_wasm_from_storage` remains a thin caller
that normalizes stored metadata and reuses the same registration flow.

## Consequences

- The main registration method stays small enough to review quickly.
- Metadata recovery and override precedence are explicit in the helper
  names.
- The helper split creates a clearer seam for unit tests around
  preparation, recovery, and persistence.
- Callers now have one more module to navigate, but the workflow itself
  is easier to understand once the helper names are known.

## Decision summary

Extract the WASM registration helpers and keep `register_wasm` as the
orchestration point. This preserves the existing runtime behaviour while
making the registration path easier to maintain and keeping its
cyclomatic complexity below 9.
