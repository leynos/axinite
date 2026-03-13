# Testing strategy

This guide explains how axinite approaches test design and validation across
local development, continuous integration (CI), scheduled runs, and targeted
manual checks.

## 1. Purpose and operating principles

The repository uses several complementary layers rather than one monolithic
test command:

- fast local checks for everyday development;
- deterministic Rust test suites for host, tool, safety, and persistence
  behaviour;
- browser-level end-to-end (E2E) tests for the web gateway;
- feature-matrix and platform CI runs that catch backend and operating-system
  drift;
- coverage and regression-enforcement workflows that keep quality from eroding
  between fixes.

The strategy aims to keep the default path practical for contributors while
still giving maintainers stronger suites for riskier changes.

## 2. Test design in day-to-day development

### 2.1 Default expectations

The default repository expectation is that testable changes ship with
corresponding automated coverage.

- Bug fixes are expected to add regression coverage. The PR workflow enforces
  this for fix-style pull requests through
  `.github/workflows/regression-test-check.yml`.
- Shared test setup in Rust is commonly factored through `rstest` fixtures.
- Deterministic fixtures are preferred over live external dependencies where
  practical.

### 2.2 Keep the default suite self-contained

Normal local test execution should not require external infrastructure.

- The default `cargo test` and `make test` path is expected to work without a
  live PostgreSQL instance.
- Tests that genuinely require external services are expected to declare that
  explicitly, typically through feature-gated integration tests.
- `scripts/check-boundaries.sh` includes a repository-level check that files in
  `tests/` which connect to PostgreSQL or depend on `DATABASE_URL` are gated
  behind `#![cfg(all(feature = "postgres", feature = "integration"))]`.
- The same boundary check also rejects silent skip patterns in tests. The
  intended model is explicit feature gating, not hidden early returns when a
  dependency is missing.

### 2.3 Test shapes used in the repository

The repository uses several recurring test forms.

Table 1. Main test forms and where they fit.

<!-- markdownlint-disable MD013 -->
| Test form | Typical location | Primary purpose |
| --------- | ---------------- | --------------- |
| Unit tests | inline `#[cfg(test)]` modules under `src/` | Verify small behaviour, invariants, and edge cases close to the implementation |
| Integration tests | `tests/*.rs` | Exercise public runtime behaviour across modules |
| Feature-gated integration tests | `tests/*.rs` with PostgreSQL or other external dependencies | Exercise paths that require backend-specific or service-specific setup |
| Browser E2E tests | `tests/e2e/` | Validate the web gateway and user-visible flows against a live binary |
| Trace-driven end-to-end tests | Rust tests backed by `tests/fixtures/llm_traces/` | Replay deterministic agent loops without calling a live LLM |
| Channel/tool-specific tests | standalone crates such as `channels-src/telegram/` | Verify extension and channel behaviour outside the root crate |
<!-- markdownlint-enable MD013 -->

### 2.4 Determinism-first test assets

Two deterministic strategies appear repeatedly in this repository.

- Python/Playwright E2E scenarios run against a real axinite binary but use a
  mock LLM server rather than a live provider.
- Rust end-to-end and replay tests use canned trace fixtures under
  `tests/fixtures/llm_traces/`, organized into `spot/`, `coverage/`, and
  `advanced/` categories.

The trace-fixture system exists to exercise the agent loop, tool dispatch, and
context accumulation without external model variance. The browser E2E suite
uses the same principle at the HTTP and DOM layer: real runtime, fake upstream
provider.

### 2.5 Manual and ignored tests

Some tests are intentionally excluded from the default path because they need
manual setup or environmental control.

- `tests/sighup_reload_integration.rs` is marked `#[ignore]` and documents the
  explicit manual setup needed before running it.
- `tests/heartbeat_integration.rs` documents its ignored/manual invocation
  pattern.
- Browser scenarios can be run one file at a time for focused debugging.

The repository therefore distinguishes between:

- default suites that should run in normal development and CI;
- optional focused suites for subsystem work;
- manual or ignored tests for specialised validation.

## 3. Local test execution

### 3.1 Standard local commands

The `Makefile` is the primary local execution surface.

Table 2. Core local validation commands.

<!-- markdownlint-disable MD013 -->
| Command | Purpose |
| ------- | ------- |
| `make check-fmt` | Check Rust formatting for the root workspace and the GitHub WASM tool crate |
| `make typecheck` | Run `cargo check` across default, libSQL-only, and all-features host configurations |
| `make lint` | Run `cargo clippy` with warnings denied across the same feature matrix |
| `make test` | Build the GitHub WASM tool, run `cargo nextest run --workspace`, and run the GitHub tool crate tests |
| `make all` | Run `check-fmt`, `typecheck`, `lint`, and `test` together |
| `make test-matrix` | Run a broader host test matrix, including default, libSQL-only, and explicit `postgres,libsql,html-to-markdown` coverage |
| `make test-cargo` | Use plain `cargo test` instead of `cargo-nextest` for harness comparison |
| `make test-matrix-cargo` | Run the broader matrix with `cargo test` rather than `cargo-nextest` |
<!-- markdownlint-enable MD013 -->

The developer guide remains the canonical reference for prerequisites and
tooling setup needed before these commands succeed.

### 3.2 Local bootstrap path

`scripts/dev-setup.sh` provides a one-shot bootstrap path for a fresh checkout.
It:

- installs the `wasm32-wasip2` target;
- ensures `wasm-tools` is available;
- runs `cargo check`;
- runs `cargo test`;
- installs the repository git hooks.

This script is intentionally oriented toward a contributor machine that does
not yet have Docker, PostgreSQL, or any other external service configured.

### 3.3 Focused local execution

The repository supports narrower execution when only one surface needs
attention.

- `cargo test test_name` runs a specific Rust test by name.
- `cargo test --manifest-path channels-src/telegram/Cargo.toml -- --nocapture`
  exercises the Telegram channel crate directly.
- `cargo test --all-features wit_compat -- --nocapture` exercises WIT
  compatibility and host-linking behaviour.
- `pytest tests/e2e/ -v` runs the browser E2E suite locally.
- `pytest tests/e2e/scenarios/test_chat.py -v` or another single scenario path
  narrows browser debugging to one flow.
- `HEADED=1 pytest tests/e2e/scenarios/test_connection.py -v` runs a browser
  test visibly for UI debugging.

### 3.4 Database-backed local work

The default local path is self-contained, but some work benefits from a local
PostgreSQL instance with `pgvector`.

- PostgreSQL-backed work uses `DATABASE_URL`.
- The developer guide documents the expected local setup for `createdb` and
  `CREATE EXTENSION IF NOT EXISTS vector;`.
- Coverage jobs and some all-features validation paths depend on PostgreSQL.

## 4. Continuous integration strategy

### 4.1 Code-style gates

`.github/workflows/code_style.yml` handles formatting and lint enforcement.

- Linux formatting checks run `cargo fmt --all -- --check`.
- Linux Clippy runs across default, libSQL-only, and all-features
  configurations with `-D warnings`.
- Windows Clippy mirrors that matrix for main-bound PRs.
- A roll-up `code-style` job turns the matrix into one branch-protection
  signal.

### 4.2 Core test matrix

`.github/workflows/test.yml` is the main automated test workflow.

- Linux host tests run via `cargo nextest run --workspace` across three
  configurations: default, libSQL-only, and explicit all-features.
- The workflow builds the GitHub WASM tool before tests that depend on its
  metadata and schema.
- The workflow also rebuilds WASM channels for integration coverage.
- Separate jobs cover Telegram channel tests, Windows compilation, WIT
  compatibility, Docker image buildability, and extension version-bump checks.
- A `run-tests` roll-up job is used as the branch-protection target.

### 4.3 Coverage jobs

`.github/workflows/coverage.yml` runs on pushes to `main`.

- Rust coverage uses `cargo-llvm-cov` across all-features, default, and
  libSQL-only configurations.
- PostgreSQL-backed coverage jobs start a `pgvector/pgvector:pg16` service and
  run migrations before test execution.
- E2E coverage builds an instrumented libSQL-only binary, runs the browser
  suite, and uploads a separate `e2e` coverage report.
- Coverage is uploaded to Codecov with separate flags for the feature-matrix
  coverage and the E2E coverage lane.

### 4.4 Browser E2E workflow

`.github/workflows/e2e.yml` covers the web gateway and user-facing browser
flows.

- It can be triggered by `workflow_call`, by a path-filtered pull request on
  `src/channels/web/**` or `tests/e2e/**`, by weekly schedule, and by manual
  dispatch.
- It compiles the binary once, uploads it as an artifact, and then fans test
  slices out in parallel.
- The scenario groups are currently `core`, `features`, and `extensions`.
- Failure screenshots are uploaded as artifacts for debugging.

### 4.5 Regression enforcement

`.github/workflows/regression-test-check.yml` is not a test runner, but it is
part of the testing strategy because it enforces the expectation that fix PRs
carry test changes.

- It activates for pull requests.
- It detects fix-style PRs from the title or commit subjects.
- It allows explicit opt-out through the `skip-regression-check` label or the
  `[skip-regression-check]` commit-message marker.
- It exempts docs-only and static-only changes.
- It fails the PR if a fix appears to land without any accompanying test
  changes.

## 5. Periodic suites

At the time of writing, the repository has one explicit scheduled test suite in
CI:

- `.github/workflows/e2e.yml` runs weekly on Monday at 06:00 UTC.

This periodic run is useful because it exercises the browser-facing runtime
outside the narrow set of pull requests that touch web code or E2E fixtures.

Coverage runs on pushes to `main`, but it is not currently scheduled as a
time-based periodic workflow.

## 6. Ad hoc and specialised validation

The repository also provides focused validation tools that are useful outside
the main PR gates.

### 6.1 Coverage helper

`scripts/coverage.sh` provides a local coverage path based on
`cargo-llvm-cov`.

- By default it runs library tests only for speed.
- It can filter to specific tests or modules.
- It supports HTML, text, JSON, and LCOV output.
- `COV_ALL_TARGETS=1` expands from the fast library-only path to include
  integration targets.

### 6.2 Boundary and policy checks

`scripts/check-boundaries.sh` is an ad hoc repository-consistency validator.
Among other checks, it verifies:

- external-service integration tests are feature-gated correctly;
- silent test-skip patterns are not used;
- other architectural boundaries are not being violated while test code grows.

### 6.3 Focused browser diagnostics

The E2E suite itself is a useful ad hoc debugging surface.

- Individual scenarios can be run directly with `pytest`.
- `HEADED=1` allows visible browser debugging.
- failure screenshots are preserved in CI;
- `tests/e2e/README.md` and `tests/e2e/CLAUDE.md` document fixtures, mock
  services, and debugging flow in detail.

### 6.4 Manual health and smoke checks

The repository also exposes validation paths that are not full test suites but
are still valuable during investigation.

- `ironclaw doctor` runs active health diagnostics against the local
  environment and configuration.
- Docker image build jobs in CI provide a packaging-level smoke test.
- WIT compatibility tests and standalone channel builds provide focused
  extension-surface validation when WASM contracts change.

## 7. Practical test-selection guidance

The intended workflow is layered.

Table 3. Suggested validation depth by change type.

<!-- markdownlint-disable MD013 -->
| Change type | Recommended local path |
| ----------- | ---------------------- |
| Small Rust logic change with no feature impact | `make check-fmt`, `make lint`, targeted Rust test, then `make test` if broader impact exists |
| Backend or feature-flag change | `make all` or at minimum `make typecheck`, `make lint`, and `make test-matrix` |
| WIT, WASM tool, or channel work | relevant local Rust tests plus `make build-github-tool-wasm`, channel build scripts, and WIT compatibility checks |
| Web gateway work | relevant Rust tests plus `pytest tests/e2e/` or a narrowed E2E scenario set |
| Bug fix | matching regression test plus the normal validation path for the touched subsystem |
| Coverage investigation | `scripts/coverage.sh` or the CI coverage workflow model |
<!-- markdownlint-enable MD013 -->

The important pattern is not "run everything, every time". The strategy is to
keep the default path fast enough for normal work, keep higher-confidence paths
available for risky changes, and preserve deterministic suites so failures are
actionable rather than noisy.
