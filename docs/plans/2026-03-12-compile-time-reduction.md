# Reduce Compile And Test Cycle Time

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`,
`Surprises & Discoveries`, `Decision Log`, and
`Outcomes & Retrospective` must be kept up to date as work proceeds.

Status: IN PROGRESS

## Purpose / big picture

After this effort, IronClaw developers should be able to edit the Rust
host, WebAssembly (WASM) extensions, and continuous integration (CI)
workflows with less waiting between
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

1. Linux and Windows Subsystem for Linux (WSL) contributors have a
   documented local `mold` setup and can verify it before measuring
   anything else.
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
  points, uses `cargo-nextest` for the root crate host path, and keeps
  explicit `cargo test` fallbacks for comparison and standalone WASM
  crates.
- `build.rs`, which now embeds registry metadata only and no longer
  forces a nested Telegram WASM build during normal host compilation.
- `scripts/build-wasm-extensions.sh`, which loops over registry
  manifests and now uses a shared `target/wasm-extensions/` cache for
  standalone extension builds.
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

The current `libsql` path also pulls duplicate generations of key
Hypertext Transfer Protocol (HTTP) and async stacks, including
`axum 0.6` and `0.8`, `tower-http 0.4` and `0.6`, and
`tokio-rustls 0.25` and `0.26`. Those duplicates are not automatically
wrong, but they are part of the compile volume.

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
  registry manifest semantics, or runtime feature behaviour unless that
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
- Interface: if reducing compile time requires changing public
  command-line interface (CLI) behaviour, public configuration keys, or
  registry manifest schemas,
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

The repository started this effort with `cargo test` directly in
`Makefile` and in CI workflows. `cargo-nextest` should become the
default host-side test runner for the root crate because it is faster,
has better failure reporting, and makes it easier to control
concurrency and retries where needed.

This milestone does not require every Rust-related test in the
repository to use `cargo-nextest` immediately. Standalone WASM tool
crates, tests that rely on unsupported harness behaviour, or specific
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
preserves release behaviour:

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
rebuild the same Rust target shapes independently. Use the end-to-end
(E2E) workflow as the model: build the binary or WASM artifacts once,
upload them, and
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
- [x] 2026-03-12 11:04Z: Proved `cargo-nextest` compatibility for the
  root crate `libsql` slice with
  `cargo nextest run --workspace --no-default-features --features libsql`.
  The run compiled in `2m 20s`, then ran `3166` tests across `42`
  binaries with `3166 passed` and `4 skipped`.
- [x] 2026-03-12 11:04Z: Updated `Makefile`,
  `.github/workflows/test.yml`, and `docs/developers-guide.md` so the
  root crate host path now uses `cargo-nextest`, while the standalone
  GitHub WASM tool crate remains on `cargo test`.
- [x] 2026-03-12 11:04Z: Re-ran `make test` after the migration. The
  new default path compiled the default-feature root crate in `6m 22s`,
  then ran `3186` tests across `43` binaries with `3186 passed` and
  `6 skipped`; the follow-on `tools-src/github` suite also passed with
  `5` tests.
- [x] 2026-03-12 11:30Z: Removed the Telegram build from `build.rs` and
  moved the remaining release-oriented behaviour to explicit scripts and
  documentation. `scripts/build-all.sh` now rebuilds channels
  explicitly, and the developer-facing docs no longer claim that
  `cargo build` auto-bundles Telegram.
- [x] 2026-03-12 11:30Z: Cleaned the host and Telegram build outputs,
  then re-ran the `libsql` timing path. The post-change command
  finished in `1:25.45`, peaked at `2351212` KB RSS, and wrote
  `target/cargo-timings/cargo-timing-20260312T111017.842651318Z.html`
  without triggering a nested channel build.
- [x] 2026-03-12 11:30Z: Proved the explicit artifact path still works
  with `./scripts/build-wasm-extensions.sh --channels`, which rebuilt
  `discord`, `slack`, `telegram`, and `whatsapp` successfully after the
  clean. Re-ran the full gate set afterward, including `make test`,
  with all tests still passing.
- [x] 2026-03-12 11:53Z: Switched
  `./scripts/build-wasm-extensions.sh` to a shared
  `target/wasm-extensions/` target dir and taught the artifact resolver
  plus local override sync script to find that cache without requiring
  `CARGO_TARGET_DIR` to stay exported in later shells.
- [x] 2026-03-12 11:53Z: Measured the shared extension cache directly.
  After deleting `target/wasm-extensions/`, a cold
  `./scripts/build-wasm-extensions.sh --channels` run finished in
  `56.14s` with `526136` KB RSS. The immediate warm rerun finished in
  `0.99s` with `34560` KB RSS.
- [x] 2026-03-12 11:53Z: Re-ran the full gate set after the shared
  target-dir change. `make test` now passes with `3187` root-crate
  tests and `6` skips, plus the `tools-src/github` suite with `5`
  passing tests.
- [x] 2026-03-12 11:57Z: Aligned workflow tool installation with the
  new branch reality. `test.yml`, `coverage.yml`, and the release WASM
  packaging job now install the required `cargo-nextest` or
  `cargo-component` tools explicitly instead of relying on runner state
  or ad hoc shell checks.
- [x] 2026-03-12 11:57Z: Updated the release WASM packaging job to use
  the shared `target/wasm-extensions/` cache when resolving built
  artifacts, while keeping a legacy per-crate `target/` fallback so the
  workflow stays compatible during the transition.
- [x] 2026-03-12 13:31Z: Applied review follow-up fixes across the
  artifact resolver, local override sync script, `Makefile`, and docs.
  The follow-up now treats an empty `CARGO_TARGET_DIR` as unset, uses
  an `rstest` fixture for the shared target-dir tests, keeps the shell
  sync logic DRY, and aligns the plan and developer guide with acronym,
  pronoun, and en-GB wording rules.
- [ ] Implement Milestone 4 and record fresh evidence.

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
- `cargo-nextest` did not require targeted fixes for the root crate on
  this branch. Both the `libsql` slice and the default-feature
  `make test` path passed cleanly once the GitHub WASM artifact was
  prebuilt.
- `src/channels/wasm/bundled.rs` already treats channels as explicit
  on-disk artifacts and falls back to the normal build tree. The flat
  `channels-src/telegram/telegram.wasm` file from `build.rs` was not
  required for normal host compilation.
- The representative `libsql` timing path still spends significant time
  in `wasmtime`, `wasmtime-wasi`, `cranelift-codegen`, and the main
  `ironclaw` crate, which means linker improvements alone will not solve
  the broader feedback-loop problem.
- The standalone WASM tool and channel crates still declare their own
  `[workspace]` roots, which preserves release isolation but also limits
  dependency reuse.
- The expensive part of the current default-feature test path is still
  the single large `ironclaw` test crate compile, not nextest
  execution. The actual `nextest` runtime was about `30.7s` after a
  `6m 22s` compile.
- Removing the hidden Telegram build reduced the representative
  `libsql` check command from `1m 58.05s` to `1m 25.45s` on this branch
  after cleaning the host and Telegram build outputs. Peak RSS stayed
  roughly flat, which implies the win is mostly removed build work
  rather than lower memory pressure.
- A shared extension target dir only helps if the discovery side can
  find it later. The artifact resolver and local override sync script
  both needed to learn about `target/wasm-extensions/` before the build
  script change would be durable outside the build shell.
- The best lookup order is not "shared first." Developers can still
  build a single channel directly into its own crate-local `target/`
  tree, so artifact discovery now checks `CARGO_TARGET_DIR` first, then
  the per-crate `target/`, then the repo-shared
  `target/wasm-extensions/` cache.
- CI had the same problem in a different form: the release packaging job
  still searched only `source_dir/target/...` even after the bulk build
  path moved to a shared target dir. Tool-install consistency and
  artifact lookup consistency have to move together.

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
- 2026-03-12 11:04Z: Switched the root crate host test path to
  `cargo-nextest` in `Makefile` and the Linux `tests` workflow, but
  kept the standalone GitHub WASM tool crate on `cargo test`.
  Rationale: local parity was proven for both the `libsql` slice and
  the default-feature `make test` path, while the separate manifest
  still benefits from the simpler legacy harness and explicit fallback.
- 2026-03-12 11:30Z: Removed the Telegram build from `build.rs`
  entirely instead of hiding it behind a new env var or feature gate.
  Rationale: the runtime, CI, and release paths already consume explicit
  channel artifacts, so keeping any implicit build-script packaging path
  would preserve the wrong default and the wrong rebuild triggers.
- 2026-03-12 11:30Z: Changed `scripts/build-all.sh` to call
  `./scripts/build-wasm-extensions.sh --channels` instead of a
  Telegram-only helper. Rationale: once the build script no longer
  manufactures a flat Telegram artifact, the explicit release-oriented
  path should rebuild the registered channels consistently.
- 2026-03-12 11:53Z: Adopted `target/wasm-extensions/` as the default
  shared target dir for `./scripts/build-wasm-extensions.sh`, while
  still honoring an explicit `CARGO_TARGET_DIR` override. Rationale:
  this captures cross-crate dependency reuse without moving the
  standalone extension crates back into the root workspace.
- 2026-03-12 11:53Z: Kept crate-local `target/` lookup ahead of the
  repo-shared cache when no env override is present. Rationale: direct
  one-off channel builds should remain immediately discoverable even if
  a shared cache from an earlier bulk build also exists.
- 2026-03-12 11:57Z: Switched the workflow tool installs to
  `taiki-e/install-action` where appropriate and updated the release
  WASM job to use the shared target dir. Rationale: workflow
  prerequisites should be declarative and consistent across jobs,
  especially once build behaviour depends on `cargo-nextest` and
  `cargo-component` rather than ambient runner state.

## Outcomes & Retrospective

This plan is now in implementation. Milestones 0, 1, 2, and 3 have
working code plus local gate evidence.

The important framing decision is that the compile-time reduction effort
should not start by changing many unrelated systems at once. The
prerequisite state was normalized first: document the local `mold`
setup, install the agreed measurement tools, and adopt
`cargo-nextest` in a controlled way. That migration is now in place for
the root crate host path and the main Linux test workflow. After that,
the hidden packaging work in `build.rs` was removed, which turned the
Telegram channel back into an explicit artifact concern and cut the
representative `libsql` check path materially. Extension builds now
also share a repo-local target cache, which cut the cold channel-build
path materially and made the warm rerun effectively instant. The next
biggest likely win is to reduce duplicated CI compilation.

The immediate next step is Milestone 4: collapse duplicated CI
compilation so the explicit channel builds and host test surfaces are
compiled once per workflow and then fanned out where practical.
