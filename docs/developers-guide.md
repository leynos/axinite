# Developer's Guide

This guide explains the local prerequisites for working on IronClaw and
reproducing the build and test workflows on this branch.

For the current system architecture and subsystem boundaries, see
[`docs/axinite-architecture-overview.md`](axinite-architecture-overview.md).

## Purpose

Linux continuous integration (CI) on this branch now uses `mold` to
reduce linker time. The
compile-time reduction plan assumes local contributors can reproduce
that setup before they measure anything or change build defaults.

This guide documents the required and optional tools for common
workflows so contributors do not discover missing prerequisites halfway
through a build.

## Supported environments

The repository builds on Linux, macOS, Windows, and Windows Subsystem
for Linux (WSL). The fastest documented path today is Linux or WSL
because the current branch already uses `mold` in Linux CI.

For compile-time or CI changes, prefer Linux or WSL so local results
line up with the current CI setup.

## Required tools

Install these tools before running the standard repository commands:

1. Rust `1.92` via `rustup`.
2. `clang` on Linux or WSL.
3. `mold` on Linux or WSL.
4. The `wasm32-wasip2` Rust target.
5. `wasm-tools`.
6. `cargo-component`.
7. `cargo-nextest`.
8. `jq`.
9. `make`.
10. Git.

The root crate declares `rust-version = "1.92"` in `Cargo.toml`. The
repository also includes standalone WebAssembly (WASM) tool and channel
crates, so WASM tooling is required for more than release-only
workflows.

## Extra tools for the compile-time reduction effort

Install these extra tools for work on the compile-time reduction plan:

1. `/usr/bin/time` or an equivalent timing tool.

`cargo-nextest` is now part of the standard local test path on this
branch because `make test` uses it for the root crate. The timing tool
remains specific to the compile-time reduction work.

## Optional tools by workflow

These tools are not required for every contributor, but they are needed
for specific work:

- PostgreSQL 15 or newer with `pgvector` for work on the default
  feature set, integration tests, or coverage jobs that use the
  PostgreSQL-backed configuration.
- Docker for container builds, worker-mode changes,
  or Docker-based validation.
- Python 3.12 plus Playwright for work on `tests/e2e` or the end-to-end
  (E2E) coverage workflow.
- `cargo-llvm-cov` for local coverage work.

## Linux and WSL setup

On Linux or WSL, install the required system packages first. The exact
package manager varies by distribution, but the important pieces are:

- `clang`
- `mold`
- `pkg-config`
- OpenSSL development headers
- `cmake`
- `gcc` and `g++`
- `jq`
- `make`

After the system packages are present, install the Rust-side tooling:

```bash
rustup toolchain install stable
rustup default stable
rustup target add wasm32-wasip2
cargo install wasm-tools --locked
cargo install cargo-component --locked
cargo install cargo-nextest --locked
```

For local coverage support:

```bash
cargo install cargo-llvm-cov --locked
```

## Local mold configuration

The repository now checks in Linux linker settings in
`.cargo/config.toml`:

```toml
[target.x86_64-unknown-linux-gnu]
linker = "clang"
rustflags = ["-C", "link-arg=-fuse-ld=mold"]
```

That means Linux and WSL contributors only need to install `clang` and
`mold` locally. Cargo will pick up the linker configuration
automatically for `x86_64-unknown-linux-gnu`.

Matching shell exports are only needed to override the checked-in
defaults. Do not assume this setting applies on macOS or Windows.

A quick verification command is:

```bash
sed -n '1,40p' .cargo/config.toml
```

## Repository bootstrap

From the repository root:

```bash
git branch --show-current
make check-fmt
make typecheck
make lint
make test
```

The current `Makefile` also includes:

- `make build-github-tool-wasm` to build the GitHub WASM tool used by
  schema and metadata tests.
- `make test-matrix` to run the broader host test combinations.
- `make test-cargo` and `make test-matrix-cargo` to keep the old
  `cargo test` path available when a harness comparison is needed for
  the root crate.
- `./scripts/build-wasm-extensions.sh --channels` to rebuild all
  registered channels into the shared `target/wasm-extensions/` cache.
- `make clean` to remove Cargo build outputs for the root crate and the
  GitHub tool crate.

## Configuration snapshots with EnvContext

The configuration system now supports an explicit snapshot model through
`crate::config::EnvContext`. Use it whenever a caller already knows the
exact environment inputs that should participate in config resolution.

The intended call pattern is:

1. Capture ambient inputs once at the application boundary with
   `EnvContext::capture_ambient()`.
2. Optionally inject secret overlays into that snapshot with
   `inject_llm_keys_into_context(...)` and
   `inject_os_credentials_into_context(...)`.
3. Build config through `Config::from_context(...)` or
   `Config::from_context_with_toml(...)`.

This keeps config resolution deterministic because the policy layer reads
from an explicit snapshot instead of touching ambient process state while
it resolves individual sub-configs.

Use the older ambient entrypoints only when the caller genuinely wants
them to do the capture work:

- `Config::from_env*` captures process env and bootstrap overlays for
  early startup paths.
- `Config::from_db*` combines DB-backed settings with an ambient env
  snapshot.
- `Config::from_context*` should be preferred in tests, pure setup code,
  and any flow that already owns a stable input snapshot.

For tests, prefer the helpers in `src/testing/test_utils.rs` or
`Config::for_testing(...)` instead of mutating `std::env`. That keeps
tests independent of host machine secrets, keychains, and shell state.

## Fast local validation loop

For quick host-side iteration on Linux or WSL with the current branch
assumptions:

```bash
set -o pipefail
/usr/bin/time -f 'ELAPSED %E\nMAXRSS_KB %M' \
  cargo check --no-default-features --features libsql --timings \
  2>&1 | tee /tmp/check-ironclaw-$(git branch --show-current | tr '/' '-').out
```

The standard fast host-side test path is now:

```bash
set -o pipefail
cargo nextest run --workspace --no-default-features --features libsql \
  2>&1 | tee /tmp/nextest-ironclaw-$(git branch --show-current | tr '/' '-').out
```

To compare behaviour against the legacy harness, use `make test-cargo`
or `make test-matrix-cargo`.

## Database-backed work

For work on the default feature set or PostgreSQL-backed tests, prepare
a local database with `pgvector` enabled:

```bash
createdb ironclaw
psql ironclaw -c "CREATE EXTENSION IF NOT EXISTS vector;"
```

Then set the database connection variable:

Variable: `DATABASE_URL`
Meaning: PostgreSQL connection URL used by the app.
Default or rule:
Required for PostgreSQL-backed work. For local development,
`postgres://localhost/ironclaw` is a typical example; include the correct
user, password, host, port, and database name when a local setup requires
them.

Example:

```bash
export DATABASE_URL=postgres://localhost/ironclaw
```

Adjust the connection string if the local PostgreSQL instance requires a
different host, user, or password.

## End-to-end (E2E) prerequisites

For browser-based tests:

```bash
python3 --version
cd tests/e2e
pip install -e .
playwright install --with-deps chromium
```

The CI E2E workflow currently builds the binary once, uploads it, and
fans test slices out from that artifact. That is the closest existing
example of the faster compile-once, fan-out pattern the compile-time
reduction effort should reuse elsewhere.

## WASM-specific notes

The repository contains standalone WASM tool and channel crates. Normal
host commands such as `cargo check`, `make typecheck`, and `make test`
no longer auto-build Telegram or other channels from `build.rs`.

The WASM toolchain is still required when intentionally building
extensions because:

- the GitHub WASM tool is built explicitly by `make build-github-tool-wasm`,
- channel build scripts rely on `cargo-component` and `wasm-tools`,
- some CI and release paths rebuild channels or tools as part of
  validation.

When WIT files, standalone extension crates, or channel code change,
expect the WASM toolchain requirements to apply even if the main focus
is the Rust host crate. Common explicit commands are:

- `./scripts/build-wasm-extensions.sh --channels` for all registered
  channels; by default it reuses the shared
  `target/wasm-extensions/` target dir,
- `./channels-src/telegram/build.sh` for a deployable Telegram channel
  artifact with `telegram.wasm`.

## When to use cargo test versus cargo-nextest

Today:

- repository defaults such as `make test` and `make test-matrix` use
  `cargo-nextest` for the root crate,
- focused standalone WASM crate checks still use `cargo test`,
- the GitHub WASM tool crate still uses `cargo test` from the standard
  repository targets.

For the compile-time reduction effort:

- treat `cargo-nextest` as the normal host-side runner for the root
  crate,
- use `make test-cargo` or `make test-matrix-cargo` when comparison
  against the old harness is needed,
- do not assume standalone WASM crates or every focused test path has
  migrated away from `cargo test`.

## Troubleshooting

- If `cargo` says `wasm32-wasip2` is missing, rerun
  `rustup target add wasm32-wasip2`.
- If builds fail because `wasm-tools` or `cargo-component` is missing,
  reinstall them with `cargo install ... --locked`.
- If local Linux or WSL timings look much slower than CI, verify that
  `clang` and `mold` are installed and that `.cargo/config.toml` is
  present before drawing conclusions.
- If PostgreSQL-backed tests fail on connection, rerun them with
  `--no-default-features --features libsql` until the local database is
  ready.
- If Playwright is missing browsers, rerun
  `playwright install --with-deps chromium`.

## Hot-reload architecture

The `src/reload/` module provides hot-reload capabilities for configuration,
HTTP listeners, and secrets without restarting the application. This is
triggered by the Unix hangup signal (SIGHUP) in production environments.

### Core traits

Four trait boundaries separate reload policy from I/O:

- `ConfigLoader` — Loads configuration from database (`DbConfigLoader`) or
  environment variables (`EnvConfigLoader`).
- `ListenerController` — Controls HTTP listener restarts, implemented by
  `WebhookListenerController` for the webhook server.
- `SecretInjector` — Injects secrets into an environment variable overlay,
  implemented by `DbSecretInjector` for database-backed secrets.
- `ChannelSecretUpdater` — Propagates rotated or cleared channel secrets
  without restarting the channel. `HotReloadManager` uses this boundary in
  step 4 of the reload flow to fan out webhook-secret changes to live
  channels.

Each trait has a native async sibling (`NativeConfigLoader`,
`NativeListenerController`, `NativeSecretInjector`,
`NativeChannelSecretUpdater`) that returns `impl Future` rather than boxed
futures. A blanket implementation converts the native traits to the
dyn-compatible boxed-future form.

### HotReloadManager orchestrator

`HotReloadManager` composes the four boundaries and coordinates the
reload sequence:

1. Inject secrets into the environment overlay
2. Load new configuration
3. Restart the HTTP listener if the bind address changed, restart a
   stopped listener when `channels.http` is present, or call `shutdown()`
   when `channels.http` is removed, so the live listener is torn down cleanly
4. Update channel secrets

The manager is created via `create_hot_reload_manager()` which wires
together the default implementations based on available stores.

### Extension guidance

Adding a new config source:

1. Implement `NativeConfigLoader` for the type.
2. The blanket impl automatically provides `ConfigLoader`.
3. Pass the loader to `HotReloadManager::new()`.

Adding a new listener controller:

1. Implement `NativeListenerController` for the server wrapper.
2. Implement `current_addr()`, `is_running()`, `restart_with_addr()`, and
   `shutdown()`.
3. Expect `shutdown()` to be called when hot reload removes the HTTP
   channel configuration, even if the listener address itself did not
   change beforehand.

### Test stubs

The `src/reload/test_stubs.rs` module provides hand-rolled stubs for
testing:

- `StubConfigLoader` — Returns a pre-configured config or error.
- `StubListenerController` — Records restart calls, can simulate failures.
- `StubSecretInjector` — Records whether `inject()` was called.
- `SpySecretUpdater` — Records all secret update calls.

Use these in unit tests to verify manager behaviour without real I/O.
Example usage is in `src/reload/manager/tests.rs`.

## WASM tool schema normalisation

WASM tools carry a parameter schema that describes their inputs to the
language model (LLM). The canonical normalization logic lives in
[src/tools/registry/schema.rs](../src/tools/registry/schema.rs).

### When `normalized_schema` returns `None`

[`schema::normalized_schema`] converts a raw `serde_json::Value` into
`Option<Value>`, returning `None` when the stored schema is effectively
missing:

- JSON `Null`.
- Empty or whitespace-only strings.
- Case-insensitive `"null"` strings.
- Placeholder schemas produced by the guest at initial registration
  (matched by `is_placeholder_schema`).

When normalisation yields `None`, the registration path falls through to
guest-export recovery. The host asks the compiled WASM component for
its own exported metadata.

### Two-phase registration flow

1. **`register_wasm`** — the lower-level entry point. Accepts raw WASM
   bytes, a pre-compiled runtime, and optional description/schema
   overrides. Compiles the component, recovers guest metadata when
   overrides are absent, and registers the tool.

2. **`register_wasm_from_storage`** — the database-driven entry point.
   Loads the stored tool record and binary with integrity verification,
   normalises description and schema via `normalized_schema` and
   `normalized_description`, then delegates to `register_wasm`.

The storage path is the one that exercises schema normalisation because
backends may persist placeholder or null schemas that must be stripped
before the guest-export recovery logic can run.

Treat `src/tools/registry/schema.rs` as the source of truth for the
exact placeholder patterns and parsing rules. If the host starts
accepting or rejecting new schema shapes, update that module and this
guide together.

## Expected follow-up changes

This guide documents the environment as of the current branch. The
compile-time reduction plan is still expected to change some of the
standard commands further, especially around shared extension build
artifacts and CI duplication.

When those changes land, this guide must be updated in the same branch
so local setup instructions stay truthful.
