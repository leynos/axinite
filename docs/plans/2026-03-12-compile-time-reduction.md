# Reduce Compile And Test Cycle Time

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: IN PROGRESS

## Purpose / big picture

After this effort, IronClaw developers should be able to edit the Rust
host, WASM extensions, and CI workflows with less waiting between
changes and usable feedback. The target is not a single trick. It is a
stack of improvements that make the common check, test, and package
paths rebuild less duplicated state.

This plan treats `mold` as a prerequisite that is already adopted for
Linux CI and local x86_64 development on this branch. Subsequent
measurements must preserve that setup instead of comparing future work
against a slower linker. The plan also makes `cargo-nextest` the
intended high-throughput test runner for the root crate once
compatibility has been proven.

Observable success has four parts:

1. Linux and WSL contributors have a documented local `mold` setup and
   can verify it before measuring anything else.
2. The root crate adopts `cargo-nextest` for the fast host-side test
   path, with any exceptions documented instead of hidden.
3. Normal host compilation stops paying packaging costs such as
   Telegram channel builds unless the user explicitly requests those
   artifacts.
4. CI stops rebuilding the same WASM and Rust surfaces across multiple
   jobs when a compile-once, fan-out model would work.

## Repository orientation

The main paths for this effort are:

- `Cargo.toml`, which defines features, the `dist` profile, and the
  current workspace exclusions for standalone WASM crates.
- `Makefile`, which now provides the standard local verification entry
  points and still uses `cargo test` directly.
- `build.rs`, which embeds registry metadata and also forces a nested
  Telegram WASM build during normal host compilation.
- `scripts/build-wasm-extensions.sh`, which loops over registry
  manifests and builds each extension crate independently.
- `.github/workflows/test.yml`,
  `.github/workflows/code_style.yml`,
  `.github/workflows/coverage.yml`, and
  `.github/workflows/e2e.yml`, which now use `mold` on Linux but still
  duplicate compile and WASM build work.
- `Dockerfile`, `Dockerfile.test`, and `Dockerfile.worker`, which
  rebuild the application in container contexts with different cache
  invalidation patterns.
- `.cargo/config.toml`, which now carries the Linux `clang` plus
  `mold` linker configuration for local x86_64 builds.
- `docs/developers-guide.md`, which must stay aligned with the real
  prerequisites needed to reproduce local build and test timings.

## Current baseline

A representative local measurement on this branch ran:

```bash
set -o pipefail
/usr/bin/time -f 'ELAPSED %E\nMAXRSS_KB %M' \
  cargo check --no-default-features --features libsql --timings \
  2>&1 | tee /tmp/check-ironclaw-build-performance.out
```

That run completed in `1m 58.05s`, peaked at `2334232` KB RSS, and
wrote a timing report to:

```plaintext
target/cargo-timings/cargo-timing-20260312T080125.546007672Z.html
```

The timing report shows meaningful time in heavyweight dependencies and
in the main crate:

- `cranelift-codegen`: `11.79s`
- `wasmtime`: `7.47s`
- `wasmtime-wasi`: `19.27s`
- `libsql`: `3.38s`
- `ironclaw` library check: `63.82s`

The current `libsql` path also pulls duplicate generations of key HTTP
and async stacks, including `axum 0.6` and `0.8`, `tower-http 0.4` and
`0.6`, and `tokio-rustls 0.25` and `0.26`. Those duplicates are not
automatically wrong, but they are part of the compile volume.

## Constraints

- This plan file must remain at
  `docs/plans/2026-03-12-compile-time-reduction.md` because the user
  requested that exact path.
- The `mold` wiring already present in Linux CI must remain intact
  throughout this effort.
- Cross-platform support must stay explicit. Linux and WSL can rely on
  `mold`; Windows and macOS must continue to build without requiring
  it.
- No change in this effort may alter released WASM artifact formats,
  registry manifest semantics, or runtime feature behavior unless that
  change is separately documented and approved.
- `cargo-nextest` adoption must preserve the semantic coverage of the
  current Rust test suites. If any test path cannot run under
  `cargo-nextest`, the exception must be documented and routed through a
  deliberate fallback.
- Compile-time reductions must not weaken leak detection, capability
  enforcement, or extension compatibility checks.
- Validation commands must use repository-native entry points where
  available and must be recorded with `set -o pipefail` and `tee` logs
  under `/tmp/...`.

## Tolerances (exception triggers)

- Scope: if the first implementation pass requires more than 14 files
  or more than 700 net lines, stop and split the work into smaller
  milestones.
- Interface: if reducing compile time requires changing public CLI
  behavior, public configuration keys, or registry manifest schemas,
  stop and escalate.
- Tooling: if adopting `cargo-nextest` leaves more than 5 existing Rust
  tests failing or unsupported after targeted fixes and clear grouping,
  stop and document the incompatibilities before changing the default
  runner.
- Topology: if sharing extension build artifacts requires moving
  standalone WASM crates back into the root workspace and that breaks
  release packaging or artifact paths, stop and compare that approach
  against a shared `CARGO_TARGET_DIR`.
- Portability: if a local `mold` setup cannot be documented in a way
  that cleanly degrades on unsupported platforms, stop and split the
  Linux or WSL configuration from the generic workflow.
- Measurement: if a milestone cannot show before-and-after evidence with
  repeatable commands and logs, do not mark it complete.

## Risks

- Risk: `build.rs` may be serving packaging or onboarding paths that are
  not obvious from the build script alone.
  Severity: high
  Likelihood: medium
  Mitigation: remove the coupling only after tracing all callers and
  replacing it with an explicit artifact-producing workflow.

- Risk: `cargo-nextest` may expose tests that rely on global state,
  environment mutation, or serialized execution.
  Severity: medium
  Likelihood: high
  Mitigation: adopt it with a compatibility inventory, keep
  unsupported tests isolated, and prove parity with `cargo test`
  results before flipping defaults.

- Risk: standalone WASM extension crates may intentionally own isolated
  target directories to simplify release bundling.
  Severity: medium
  Likelihood: medium
  Mitigation: evaluate a shared `CARGO_TARGET_DIR` before workspace
  reunification, and validate artifact discovery after each change.

- Risk: a local-only `mold` setup in shell profiles can drift from CI if
  it is not documented precisely.
  Severity: medium
  Likelihood: high
  Mitigation: keep `docs/developers-guide.md` precise and include a
  simple verification command that proves which linker Cargo is using.

- Risk: CI wall-clock time can increase temporarily while the repository
  carries both old and new test runner paths during migration.
  Severity: low
  Likelihood: medium
  Mitigation: migrate one workflow at a time, keep roll-up jobs honest,
  and remove superseded paths promptly once parity is proven.

## Milestone 0: Normalize the baseline around mold

Treat `mold` as the prerequisite gate for Linux and WSL compile-time
work.

This milestone is not about rediscovering the linker win. It is about
making the existing CI optimization reproducible for human contributors
and trustworthy enough that later measurements are not compared against
a slower local setup by mistake.

Implementation steps:

1. Create `docs/developers-guide.md` with exact local prerequisites for
   Linux and WSL, including `clang`, `mold`, the Rust toolchain, the
   WASM target, and the extra tools needed for WASM extensions and
   optional test suites.
2. Add a small section documenting how a developer verifies the linker
   path locally. Prefer a checked-in Cargo configuration over shell
   profile instructions when the target-specific setup is portable
   enough to keep in the repo.
3. Record one Linux or WSL baseline command for `cargo check --timings`
   and one command for the test suite baseline after the guide is in
   place.

Do not start comparing follow-up improvements until this prerequisite
documentation exists.

## Milestone 1: Adopt cargo-nextest deliberately

Replace ad hoc reliance on `cargo test` for the common Rust host test
path with `cargo-nextest`, while keeping exceptions explicit.

The current repository uses `cargo test` directly in `Makefile` and in
CI workflows. `cargo-nextest` should become the default host-side test
runner for the root crate because it is faster, has better failure
reporting, and makes it easier to control concurrency and retries where
needed.

This milestone does not require every Rust-related test in the
repository to use `cargo-nextest` immediately. Standalone WASM tool
crates, tests that rely on unsupported harness behavior, or specific
serialized suites may retain `cargo test` temporarily, but the fallback
must be intentional and documented.

Implementation steps:

1. Install `cargo-nextest` as a documented developer prerequisite in
   `docs/developers-guide.md`.
2. Inventory the current root-crate test entry points in `Makefile`,
   `.github/workflows/test.yml`, and any focused scripts.
3. Run the current host test matrix with `cargo-nextest` and document
   any incompatible tests or required filters.
4. Update `Makefile` so the default host test target uses
   `cargo nextest run` for the root crate, while preserving clear
   fallbacks for any paths that must stay on `cargo test`.
5. Update at least one Linux CI workflow to use `cargo-nextest` after
   parity is proven locally.

Acceptance evidence should include:

```bash
set -o pipefail
BRANCH=$(git branch --show)
cargo nextest run --workspace --no-default-features --features libsql \
  2>&1 | tee /tmp/nextest-libsql-ironclaw-${BRANCH}.out
```

and an updated repository-native command path such as `make test` or a
new explicit `make nextest`.

## Milestone 2: Remove packaging work from normal compilation

Make host compilation stop rebuilding Telegram channel artifacts unless
the user is intentionally producing those artifacts.

The current `build.rs` builds Telegram WASM during normal crate
compilation. That cost leaks into `cargo check`, `cargo test`, Docker
builds, and release jobs even though `src/channels/wasm/bundled.rs`
describes channels as separately compiled artifacts. This milestone must
replace the hidden build-script side effect with an explicit packaging
path.

Implementation steps:

1. Trace every consumer of `channels-src/telegram/telegram.wasm` and
   every assumption that the file already exists.
2. Introduce a narrow gate so normal host compilation skips Telegram
   artifact production unless explicitly requested.
3. Move the required Telegram or channel build into explicit commands
   such as `scripts/build-all.sh`, release packaging, or onboarding
   preparation.
4. Update developer documentation so contributors know when they do and
   do not need to build channels.

Acceptance evidence should include before-and-after
`cargo check --timings` runs and a proof that the explicit channel build
path still produces the expected artifacts.

## Milestone 3: Share more work across extension builds

Reduce duplicated compilation across the standalone WASM tool and
channel crates.

Right now the repository excludes those crates from the root workspace
and builds them one manifest at a time. That keeps packaging boundaries
clear, but it also means repeated dependency compilation. This milestone
should compare two concrete strategies and pick the smallest one that
preserves release behavior:

1. Keep standalone crates, but set a shared `CARGO_TARGET_DIR` for
   `scripts/build-wasm-extensions.sh` and related CI jobs.
2. Move some or all extension crates into the root workspace while
   preserving artifact discovery and packaging semantics.

The preferred order is to try shared target directories first because it
is lower risk.

Acceptance evidence should include repeated runs of:

```bash
set -o pipefail
BRANCH=$(git branch --show)
./scripts/build-wasm-extensions.sh --channels \
  2>&1 | tee /tmp/build-wasm-channels-ironclaw-${BRANCH}.out
```

and either timing improvements or a clear rejection of the attempted
approach with documented reasons.

## Milestone 4: Collapse duplicated CI compilation

Change CI from repeated independent builds to compile-once, fan-out
where practical.

The most obvious duplication today is that Linux tests, coverage, and
release packaging all rebuild channel artifacts, and some workflows
rebuild the same Rust target shapes independently. Use the E2E workflow
as the model: build the binary or WASM artifacts once, upload them, and
run downstream jobs from those artifacts when the downstream jobs do not
need to recompile.

Implementation steps:

1. Build channel artifacts once per relevant Linux workflow and upload
   them.
2. Remove redundant per-matrix channel rebuild steps.
3. Revisit the PR matrix so broader feature combinations move to
   `push: main`, nightly, or other non-PR gates if branch protection
   does not need them.
4. Standardize cache usage so Rust jobs do not mix incompatible caching
   patterns without justification.

Acceptance evidence should include workflow diffs plus a summary of the
removed redundant compile steps.

## Milestone 5: Reduce container build invalidation

Narrow Docker build contexts so container validation does not recompile
the world after unrelated repo changes.

`Dockerfile.worker` currently uses `COPY . .`, and the other Dockerfiles
still copy more inputs than a normal app build should need because
`build.rs` watches channel and WIT paths. After Milestone 2 removes the
hidden Telegram build coupling, the Dockerfiles should be revisited to
copy only the files needed for each image.

Implementation steps:

1. Replace `COPY . .` in `Dockerfile.worker` with selective copies or a
   dependency-planning stage.
2. Remove `tests/`, `channels-src/`, `registry/`, or `wit/` from the
   main image build contexts if they are no longer needed by normal
   compilation.
3. Consider a shared builder base or other reuse strategy for repeated
   Rust and WASM tool installation.

Acceptance evidence should include at least one cache-aware rebuild
demonstration showing that a docs-only or workflow-only change no longer
invalidates the worker compile layer.

## Concrete steps

Work from the repository root
`/data/leynos/Projects/axinite.worktrees/build-performance`.

1. Confirm the current prerequisite state and the local baseline:

   ```bash
   git branch --show
   rg -n "mold|cargo-nextest|nextest|link-arg=-fuse-ld=mold" \
     .github Makefile Cargo.toml docs scripts
   set -o pipefail
   /usr/bin/time -f 'ELAPSED %E\nMAXRSS_KB %M' \
     cargo check --no-default-features --features libsql --timings \
     2>&1 | tee /tmp/check-ironclaw-build-performance.out
   ```

2. Create and keep `docs/developers-guide.md` current before changing
   build or test defaults.
3. Prototype `cargo-nextest` locally and document any incompatibilities
   before updating `Makefile` or CI.
4. Remove or gate the hidden Telegram build from `build.rs`, then
   remeasure.
5. Evaluate shared extension build artifacts, then remeasure.
6. Simplify CI and Docker only after the local fast path has been
   improved and documented.

## Progress

- [x] 2026-03-12 08:18Z: Audited the current branch and confirmed that
  Linux CI now uses `mold` through workflow environment variables and
  `rui314/setup-mold@v1`.
- [x] 2026-03-12 08:19Z: Confirmed that `cargo-nextest` is not yet
  adopted in `Makefile` or repository documentation.
- [x] 2026-03-12 08:20Z: Recorded the current local `libsql` timing
  baseline and identified the heaviest crates from Cargo's timing
  report.
- [x] 2026-03-12 08:21Z: Drafted this ExecPlan at the user-requested
  path.
- [x] 2026-03-12 08:27Z: Created `docs/developers-guide.md` and aligned
  it with the prerequisite assumptions in this plan, including local
  `mold`, WASM tooling, and `cargo-nextest`.
- [x] 2026-03-12 08:39Z: Added `.cargo/config.toml` so Linux and WSL
  contributors use the same `clang` plus `mold` linker setup as Linux
  CI without extra shell exports.
- [x] 2026-03-12 08:41Z: Received plan approval and started
  implementation.
- [x] 2026-03-12 08:49Z: Ran `make typecheck` with the checked-in
  linker config. The root crate and `tools-src/github` check targets
  both passed under the new local `mold` setup.
- [ ] Implement Milestone 0 validation runs and record fresh evidence.

## Surprises & Discoveries

- The rebased branch already contains Linux CI-side `mold` adoption,
  and this implementation now mirrors that setup for local
  `x86_64-unknown-linux-gnu` builds through `.cargo/config.toml`.
- The repository now has a `Makefile`, which gives a better anchor for
  future standardization than the earlier script-only baseline.
- `cargo-nextest` is absent from both the current `Makefile` and
  existing docs, so adoption will need both tooling work and
  documentation in the same change.
- The checked-in Linux linker configuration works cleanly with the
  existing repository `typecheck` gate, including the separate GitHub
  WASM tool crate.
- The representative `libsql` timing path still spends significant time
  in `wasmtime`, `wasmtime-wasi`, `cranelift-codegen`, and the main
  `ironclaw` crate, which means linker improvements alone will not solve
  the broader feedback-loop problem.
- The standalone WASM tool and channel crates still declare their own
  `[workspace]` roots, which preserves release isolation but also limits
  dependency reuse.

## Decision Log

- 2026-03-12 08:18Z: Treated `mold` as a prerequisite rather than a new
  optimization project. Rationale: the user explicitly stated the
  branch has already been rebased onto the `mold` work, and the current
  Linux CI workflows confirm that fact.
- 2026-03-12 08:19Z: Started with `docs/developers-guide.md` as the
  prerequisite anchor because the repository had CI-side linker wiring
  but no local config file yet. Rationale: documentation was the
  smallest truthful bridge while the plan was still in draft.
- 2026-03-12 08:20Z: Placed `cargo-nextest` adoption ahead of
  build-graph surgery. Rationale: test-runner improvements can reduce
  feedback time early and provide better measurement loops for later
  compile-time work.
- 2026-03-12 08:21Z: Chose shared extension target directories as the
  first strategy to test before workspace reunification. Rationale: it
  is less invasive and less likely to disturb release packaging
  semantics.
- 2026-03-12 08:27Z: Kept the developer guide focused on prerequisites
  and current branch truth, not future-state promises. Rationale:
  contributors need a truthful environment guide now, while the
  ExecPlan carries the future migration steps.
- 2026-03-12 08:39Z: Checked in `.cargo/config.toml` for
  `x86_64-unknown-linux-gnu` so local Linux or WSL builds use the same
  `clang` plus `mold` setup as Linux CI. Rationale: this removes shell
  profile drift and makes follow-up measurements reproducible.

## Outcomes & Retrospective

This plan is now in implementation. Milestone 0 has started, and later
sections will be updated as evidence is gathered.

The important framing decision is that the compile-time reduction effort
should not start by changing many unrelated systems at once. The
prerequisite state should be normalized first: document the local
`mold` setup, install the agreed measurement tools, and adopt
`cargo-nextest` in a controlled way. After that, the biggest likely
wins are to remove hidden packaging work from `build.rs`, share more
work across extension builds, and reduce duplicated CI compilation.

The immediate next step is to finish Milestone 0 validation runs using
the checked-in `.cargo/config.toml` and `docs/developers-guide.md` as
the source of truth for local prerequisites.
