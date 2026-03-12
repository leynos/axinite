# Developer's Guide

This guide explains the local prerequisites for working on IronClaw and
reproducing the build and test workflows on this branch.

## Purpose

Linux CI on this branch now uses `mold` to reduce linker time. The
compile-time reduction plan assumes local contributors can reproduce
that setup before they measure anything or change build defaults.

This guide documents the required and optional tools for common
workflows so contributors do not discover missing prerequisites halfway
through a build.

## Supported environments

The repository builds on Linux, macOS, Windows, and WSL. The fastest
documented path today is Linux or WSL because the current branch already
uses `mold` in Linux CI.

If you are working on compile-time or CI changes, prefer Linux or WSL
so your local results line up with the current CI setup.

## Required tools

Install these tools before running the standard repository commands:

1. Rust `1.92` via `rustup`.
2. `clang` on Linux or WSL.
3. `mold` on Linux or WSL.
4. The `wasm32-wasip2` Rust target.
5. `wasm-tools`.
6. `cargo-component`.
7. `jq`.
8. `make`.
9. Git.

The root crate declares `rust-version = "1.92"` in `Cargo.toml`. The
repository also includes standalone WASM tool and channel crates, so
WASM tooling is required for more than release-only workflows.

## Extra tools for the compile-time reduction effort

Install these extra tools if you are working on the compile-time
reduction plan:

1. `cargo-nextest`.
2. `/usr/bin/time` or an equivalent timing tool.

`cargo-nextest` is not yet the repository-wide default test runner on
this branch, but the compile-time reduction plan adopts it as the
intended faster host-side test runner. Install it now if you are
validating that migration.

## Optional tools by workflow

These tools are not required for every contributor, but they are needed
for specific work:

- PostgreSQL 15 or newer with `pgvector` if you are working on the
  default feature set, integration tests, or coverage jobs that use the
  PostgreSQL-backed configuration.
- Docker if you are working on container builds, worker-mode changes,
  or Docker-based validation.
- Python 3.12 plus Playwright if you are working on `tests/e2e` or the
  E2E coverage workflow.
- `cargo-llvm-cov` if you are working on coverage locally.

## Linux and WSL setup

On Linux or WSL, install the system packages you need first. The exact
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

If you also want local coverage support:

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

You do not need matching shell exports unless you want to override the
checked-in defaults. Do not assume this setting applies on macOS or
Windows.

A quick verification command is:

```bash
sed -n '1,40p' .cargo/config.toml
```

## Repository bootstrap

From the repository root:

```bash
git branch --show
make check-fmt
make typecheck
make lint
make test
```

The current `Makefile` also includes:

- `make build-github-tool-wasm` to build the GitHub WASM tool used by
  schema and metadata tests.
- `make test-matrix` to run the broader host test combinations.
- `make clean` to remove Cargo build outputs for the root crate and the
  GitHub tool crate.

## Fast local validation loop

For quick host-side iteration on Linux or WSL with the current branch
assumptions:

```bash
set -o pipefail
/usr/bin/time -f 'ELAPSED %E\nMAXRSS_KB %M' \
  cargo check --no-default-features --features libsql --timings \
  2>&1 | tee /tmp/check-ironclaw-$(git branch --show).out
```

If you are validating the future `cargo-nextest` path from the
compile-time reduction plan, use:

```bash
set -o pipefail
cargo nextest run --workspace --no-default-features --features libsql \
  2>&1 | tee /tmp/nextest-ironclaw-$(git branch --show).out
```

If `cargo-nextest` exposes test incompatibilities, keep using the
current repository targets for day-to-day work until the migration is
completed.

## Database-backed work

If you are working on the default feature set or PostgreSQL-backed
tests, prepare a local database with `pgvector` enabled:

```bash
createdb ironclaw
psql ironclaw -c "CREATE EXTENSION IF NOT EXISTS vector;"
```

Then set:

```bash
export DATABASE_URL=postgres://localhost/ironclaw
```

Adjust the connection string if your local PostgreSQL instance requires
a different host, user, or password.

## E2E prerequisites

If you are working on browser-based tests:

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

The repository contains standalone WASM tool and channel crates. Common
host-only commands can still trigger WASM-related prerequisites because:

- the GitHub WASM tool is built explicitly by `make build-github-tool-wasm`,
- channel build scripts rely on `cargo-component` and `wasm-tools`,
- some CI and release paths rebuild channels or tools as part of
  validation.

If you change WIT files, standalone extension crates, or channel code,
expect the WASM toolchain requirements to apply even if your main focus
is the Rust host crate.

## When to use cargo test versus cargo-nextest

Today:

- repository defaults such as `make test` still use `cargo test`,
- focused standalone WASM crate checks also use `cargo test`.

For the compile-time reduction effort:

- install `cargo-nextest` now,
- use it for compatibility testing and migration work on the root crate,
- do not assume every current test path has already been migrated.

## Troubleshooting

- If `cargo` says `wasm32-wasip2` is missing, rerun
  `rustup target add wasm32-wasip2`.
- If builds fail because `wasm-tools` or `cargo-component` is missing,
  reinstall them with `cargo install ... --locked`.
- If local Linux or WSL timings look much slower than CI, verify that
  `clang` and `mold` are installed and that `.cargo/config.toml` is
  present before drawing conclusions.
- If PostgreSQL-backed tests fail on connection, rerun them with
  `--no-default-features --features libsql` until your local database is
  ready.
- If Playwright is missing browsers, rerun
  `playwright install --with-deps chromium`.

## Expected follow-up changes

This guide documents the environment as of the current branch. The
compile-time reduction plan is expected to change some of the standard
commands, especially around `cargo-nextest`, hidden Telegram channel
builds, and CI duplication.

When those changes land, this guide must be updated in the same branch
so local setup instructions stay truthful.
