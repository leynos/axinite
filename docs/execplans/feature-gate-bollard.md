# Feature-Gate bollard (`docker` Feature)

**Branch:** (to be created from `build-time`)
**Date:** 2026-03-15
**Status:** Plan ready; not yet started
**Estimated impact:** ~156 fewer crates when `docker` feature is off

## Big Picture

The bollard crate (Docker API client) and its ~156 transitive dependencies
are always compiled, even though Docker-based sandboxing is an optional
deployment capability. Feature-gating it behind a `docker` feature
(included in defaults) lets developers who do not use Docker sandboxing
skip those crates.

## Constraints

- The `docker` feature must be included in `default` so that the default
  build continues to include Docker support.
- `--all-features` must still compile and pass all tests.
- The sandbox subsystem must degrade gracefully when Docker is unavailable
  at compile time (return clear errors or omit Docker-specific code paths).

## Scope of bollard Usage

bollard is used in **5 source files**, confined to the sandbox and
orchestrator modules:

Table 1. Files with direct bollard usage and primary purpose.

| File | Lines | Purpose |
|------|-------|---------|
| `src/sandbox/container.rs` | ~636 | Container lifecycle (`ContainerRunner`) |
| `src/sandbox/manager.rs` | ~525 | High-level sandbox coordinator |
| `src/sandbox/error.rs` | ~59 | Error types (`#[from] bollard::errors::Error`) |
| `src/orchestrator/job_manager.rs` | ~709 | Persistent container job management |
| `src/orchestrator/reaper.rs` | ~970 | Orphaned container cleanup |

### Public API leakage

- `SandboxError::Docker` variant exposes `bollard::errors::Error` via
  `#[from]`.
- `ContainerRunner` is public and holds a `Docker` field.
- `ContainerJobManager` stores `Arc<RwLock<Option<bollard::Docker>>>` but
  does not expose it in method signatures.

## Implementation Steps

### Phase 1: Cargo.toml changes

- [ ] Make bollard optional:

  ```toml
  bollard = { version = "0.18", optional = true }
  ```

- [ ] Add `docker` feature:

  ```toml
  docker = ["dep:bollard"]
  ```

- [ ] Add `docker` to `default` features:

  ```toml
  default = ["postgres", "libsql", "html-to-markdown", "docker"]
  ```

### Phase 2: Gate Docker modules

- [ ] In `src/sandbox/container.rs`: gate entire file with
  `#[cfg(feature = "docker")]`
- [ ] In `src/sandbox/manager.rs`: gate Docker-specific functionality
  (the module likely has non-Docker sandbox paths too — audit before
  blanket-gating)
- [ ] In `src/sandbox/error.rs`: gate the `Docker` error variant:

  ```rust
  #[cfg(feature = "docker")]
  #[error("Docker API error: {0}")]
  Docker(#[from] bollard::errors::Error),
  ```

- [ ] In `src/orchestrator/job_manager.rs`: gate Docker-specific code
  paths with `#[cfg(feature = "docker")]`
- [ ] In `src/orchestrator/reaper.rs`: gate Docker-specific code paths
  with `#[cfg(feature = "docker")]`

### Phase 3: Handle transitive references

- [ ] Audit `src/sandbox/mod.rs` exports — conditionally export
  Docker-specific types
- [ ] Audit `src/orchestrator/mod.rs` exports — conditionally export
  Docker-specific types
- [ ] Audit `src/app.rs` and `src/bootstrap.rs` for Docker setup code —
  gate with `#[cfg(feature = "docker")]`
- [ ] Check `src/config/` for Docker-specific configuration — gate or
  provide defaults

### Phase 4: Gate Docker-dependent tests

- [ ] Identify integration tests that require Docker (likely none compile
  against bollard directly, but some may test sandbox functionality)
- [ ] Gate those tests with `#[cfg(feature = "docker")]`

### Phase 5: Validate

- [ ] `make all` passes (default features, includes `docker`)
- [ ] `cargo check --no-default-features --features libsql,test-helpers`
  passes (no Docker)
- [ ] `cargo check --all-features --features test-helpers` passes
- [ ] Full test suite passes
- [ ] Verify crate count reduction: compare `cargo tree --prefix none |
  sort -u | wc -l` with and without `docker` feature

## Risks

- **Sandbox manager coupling:** The sandbox manager may interleave Docker
  and non-Docker logic (e.g., proxy setup, network policy). A full audit of
  `src/sandbox/manager.rs` is needed to determine whether the entire module
  can be gated or only specific functions.
- **Error type compatibility:** Gating the `Docker` variant of
  `SandboxError` changes the enum's shape. Any `match` on `SandboxError`
  must handle this with `#[cfg]` on the arm, or use a wildcard.
- **Orchestrator coupling:** The job manager and reaper may be tightly
  integrated with non-Docker orchestration logic. Audit before gating.

## Progress

- [ ] Phase 1: Cargo.toml changes
- [ ] Phase 2: Gate Docker modules
- [ ] Phase 3: Handle transitive references
- [ ] Phase 4: Gate Docker-dependent tests
- [ ] Phase 5: Validate
