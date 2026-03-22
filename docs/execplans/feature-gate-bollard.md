# Feature-Gate bollard (`docker` Feature)

**Branch:** (to be created from `build-time`)
**Date:** 2026-03-15
**Status:** Completed
**Measured impact:** 1 fewer crate when `docker` feature is off
  (`cargo tree --prefix none | sort -u | wc -l`: 810 with default features,
  809 with `postgres,libsql,html-to-markdown` and `docker` disabled)

## Big Picture

The bollard crate (Docker API client) and its transitive dependency surface
is always compiled, even though Docker-based sandboxing is an optional
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

The direct bollard usage sites and their primary purposes are:

- `src/sandbox/container.rs`: container lifecycle (`ContainerRunner`)
- `src/sandbox/manager.rs`: high-level sandbox coordinator
- `src/sandbox/error.rs`: error types, including
  `#[from] bollard::errors::Error`
- `src/orchestrator/job_manager.rs`: persistent container job management
- `src/orchestrator/reaper.rs`: orphaned container cleanup

### Public API leakage

- `SandboxError::Docker` variant exposes `bollard::errors::Error` via
  `#[from]`.
- `ContainerRunner` is public and holds a `Docker` field.
- `ContainerJobManager` stores `Arc<RwLock<Option<bollard::Docker>>>` but
  does not expose it in method signatures.

## Implementation Steps

### Phase 1: Cargo.toml changes

- [x] Make bollard optional:

  ```toml
  bollard = { version = "0.18", optional = true }
  ```

- [x] Add `docker` feature:

  ```toml
  docker = ["dep:bollard"]
  ```

- [x] Add `docker` to `default` features:

  ```toml
  default = ["postgres", "libsql", "html-to-markdown", "docker"]
  ```

### Phase 2: Gate Docker modules

- [x] In `src/sandbox/container.rs`: split Docker-backed internals behind
  `#[cfg(feature = "docker")]` and keep no-Docker stubs returning clear
  `DockerNotAvailable` errors.
- [x] In `src/sandbox/manager.rs`: gate Docker-specific functionality
  while preserving direct-execution paths.
- [x] In `src/sandbox/error.rs`: gate the `Docker` error variant:

  ```rust
  #[cfg(feature = "docker")]
  #[error("Docker API error: {0}")]
  Docker(#[from] bollard::errors::Error),
  ```

- [x] In `src/orchestrator/job_manager.rs`: gate Docker-specific code
  paths with `#[cfg(feature = "docker")]`
- [x] In `src/orchestrator/reaper.rs`: gate Docker-specific code paths
  with `#[cfg(feature = "docker")]`

### Phase 3: Handle transitive references

- [x] Audit `src/sandbox/mod.rs` exports — preserve module exports while
  making Docker connection types feature-aware.
- [x] Audit `src/orchestrator/mod.rs` exports — preserve exported setup
  surface and rely on runtime `None`/`Disabled` results when Docker support
  is absent.
- [x] Audit `src/app.rs` and `src/bootstrap.rs` for Docker setup code —
  no direct `bollard` references required after internal gating.
- [x] Check `src/config/` for Docker-specific configuration — existing
  defaults remain valid; no additional gating required.

### Phase 4: Gate Docker-dependent tests

- [x] Identify integration tests that require Docker (likely none compile
  against bollard directly, but some may test sandbox functionality)
- [x] Gate those tests with `#[cfg(feature = "docker")]`

### Phase 5: Validate

- [x] `make all` equivalent gates pass (default features, includes `docker`)
- [x] `cargo check --no-default-features --features libsql,test-helpers`
  passes (no Docker)
- [x] `cargo check --all-features --features test-helpers` passes
- [x] Full test suite passes
- [x] Verify crate count reduction: compare `cargo tree --prefix none |
  sort -u | wc -l` with and without `docker` feature
  Measured on 2026-03-22 as 810 crates with default features and 809 crates
  with `docker` disabled while keeping `postgres`, `libsql`, and
  `html-to-markdown` enabled.

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

## Progress Notes

- 2026-03-20: Made `bollard` optional, added a default-on `docker` feature,
  and converted the sandbox/orchestrator internals to compile without Docker
  support while still returning clear runtime errors when Docker-backed code
  paths are invoked.
- 2026-03-20: `check_docker()` now reports `Disabled` when the crate is built
  without the `docker` feature, and Docker-backed tests are gated so the
  no-Docker build has a dedicated compile path.
- 2026-03-21: The branch needed a follow-on clean-build pass because stable
  no-Docker compilation was being poisoned by `cap-*` build-script probes that
  trusted an ambient `RUSTC_WRAPPER`. The fix was vendored patching of the
  affected probe chain plus narrow configuration (`cfg`) cleanup in the
  sandbox and orchestrator modules.
- 2026-03-21: The vendored `cap-*` workaround has now been retired. Fixing the
  ambient `notdeadyet` wrapper restored honest stdin-backed compiler probes,
  and the unpatched stable no-Docker acceptance check passed in a scratch copy
  before the repository-local patches were removed.
- 2026-03-21: Validation is now complete. The branch passed
  `make check-fmt`, `make typecheck`, `make lint`, and `make test`.
  The stable no-Docker acceptance check also passes.
- 2026-03-22: The crate-count validation was closed with a fresh measurement.
  The current resolved graph drops from 810 unique crates to 809 when the
  `docker` feature is disabled while keeping the other default features.

## Progress

- [x] Phase 1: Cargo.toml changes
- [x] Phase 2: Gate Docker modules
- [x] Phase 3: Handle transitive references
- [x] Phase 4: Gate Docker-dependent tests
- [x] Phase 5: Validate
