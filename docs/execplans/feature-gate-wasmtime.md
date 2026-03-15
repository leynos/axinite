# Feature-Gate wasmtime (`wasm` Feature)

**Branch:** (to be created from `build-time`)
**Date:** 2026-03-15
**Status:** Plan ready; not yet started
**Estimated impact:** ~300 fewer crates when `wasm` feature is off

## Big Picture

The wasmtime ecosystem (wasmtime, wasmtime-wasi, wasmtime-cranelift,
wasmparser, and ~300 transitive dependencies including the Cranelift
compiler backend) is always compiled, even though WASM tool/channel
execution is an optional capability. Feature-gating it behind a `wasm`
feature (included in defaults) lets developers who are not working on WASM
functionality skip ~300 crate compilations, saving ~42 s on clean builds.

## Constraints

- The `wasm` feature must be included in `default` so that the default build
  continues to include WASM support.
- The feature matrix in `make typecheck`, `make lint`, and `make
  test-matrix` must exercise the `wasm` feature.
- The `--all-features` combo already covers it.
- WASM-dependent integration tests must be gated with
  `#[cfg(feature = "wasm")]`.

## Scope of wasmtime Usage

wasmtime/wasmtime-wasi are used in **6 source files**, all confined to two
module subtrees:

| File | Module |
|------|--------|
| `src/tools/wasm/limits.rs` | Tools WASM runtime |
| `src/tools/wasm/runtime.rs` | Tools WASM runtime |
| `src/tools/wasm/wrapper.rs` | Tools WASM wrapper |
| `src/tools/wasm/wrapper/metadata.rs` | Tools WASM metadata |
| `src/channels/wasm/runtime.rs` | Channels WASM runtime |
| `src/channels/wasm/wrapper.rs` | Channels WASM wrapper |

wasmparser is used in **1 file**:

| File | Module |
|------|--------|
| `src/tools/builder/validation.rs` | Tool builder validation |

Additional files that reference WASM types transitively (e.g., the tool
registry, extension manager, channel setup) will need conditional
compilation or stub types when the feature is off.

## Implementation Steps

### Phase 1: Cargo.toml changes

- [ ] Make wasmtime, wasmtime-wasi, and wasmparser optional:
  ```toml
  wasmtime = { version = "28", features = ["component-model"], optional = true }
  wasmtime-wasi = { version = "28", optional = true }
  wasmparser = { version = "0.220", optional = true }
  ```
- [ ] Add `wasm` feature:
  ```toml
  wasm = ["dep:wasmtime", "dep:wasmtime-wasi", "dep:wasmparser"]
  ```
- [ ] Add `wasm` to `default` features:
  ```toml
  default = ["postgres", "libsql", "html-to-markdown", "wasm"]
  ```

### Phase 2: Gate WASM modules

- [ ] In `src/tools/wasm/mod.rs`: gate the module with
  `#[cfg(feature = "wasm")]`
- [ ] In `src/channels/wasm/mod.rs`: gate the module with
  `#[cfg(feature = "wasm")]`
- [ ] In `src/tools/builder/validation.rs`: gate wasmparser usage with
  `#[cfg(feature = "wasm")]`

### Phase 3: Handle transitive references

- [ ] Audit all files that import from `tools::wasm` or `channels::wasm`
- [ ] Gate those imports and usage with `#[cfg(feature = "wasm")]`
- [ ] For public API surfaces (e.g., `Tool` enum variants, channel setup),
  add `#[cfg(feature = "wasm")]` to WASM-specific variants/branches
- [ ] Provide stub/no-op implementations or compile-time errors where WASM
  is required but the feature is off

### Phase 4: Gate WASM-dependent tests

- [ ] Gate integration tests that test WASM functionality (e.g.,
  `wit_compat.rs`, `wasm_channel_integration.rs`) with
  `#[cfg(feature = "wasm")]`

### Phase 5: Update build infrastructure

- [ ] Update Makefile `typecheck` and `lint` targets: the
  `--no-default-features --features libsql,test-helpers` combo should
  explicitly exclude `wasm` (already the case since it is not in that
  feature list)
- [ ] Verify `--all-features` still compiles and passes all tests
- [ ] Update AGENTS.md if the feature matrix documentation changes

### Phase 6: Validate

- [ ] `make all` passes (default features, includes `wasm`)
- [ ] `cargo check --no-default-features --features libsql,test-helpers`
  passes (no WASM)
- [ ] `cargo check --all-features --features test-helpers` passes
- [ ] `cargo nextest run --workspace --features test-helpers` passes
- [ ] Verify crate count reduction: compare `cargo tree --prefix none |
  sort -u | wc -l` with and without `wasm` feature

## Risks

- **Transitive type leakage:** If any public type in the non-WASM API
  surface depends on a wasmtime type, the feature gate will cause compile
  errors when `wasm` is off. These must be resolved with conditional
  compilation or type abstraction.
- **Test coverage gap:** WASM tests are only run when the feature is on.
  CI must include at least one job with `wasm` enabled (the default features
  job covers this).

## Progress

- [ ] Phase 1: Cargo.toml changes
- [ ] Phase 2: Gate WASM modules
- [ ] Phase 3: Handle transitive references
- [ ] Phase 4: Gate WASM-dependent tests
- [ ] Phase 5: Update build infrastructure
- [ ] Phase 6: Validate
