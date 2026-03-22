# Recover clean build after bollard optionality

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work
proceeds.

Status: COMPLETED AND RETIRED
Publication: final branch publication was performed by Codex on 2026-03-22.

## Purpose / big picture

The `feature-gate-bollard` branch already contains the Docker feature-gating
changes. This plan captured the follow-on work to restore a clean
stable-toolchain build, remove the no-Docker warning fallout from the new stub
paths, replay the full Axinite gate contract, and then retire the temporary
vendored `cap-*` workaround once the underlying wrapper bug was fixed.

Success is observable in these ways:

1. Running the narrow stable check below succeeds without `RUSTC_BOOTSTRAP`:

   ```bash
   cargo check --no-default-features --features libsql,test-helpers
   ```

2. Running the branch gates below succeeds with tee logs under `/tmp`:

   ```bash
   make check-fmt
   make typecheck
   make lint
   make test
   ```

3. `git status --short` shows only the intended source changes before commit,
   and the branch can then be committed and pushed.
4. The vendored `cap-*` workaround can be removed after the unpatched graph
   proves the same stable acceptance path cleanly.

## Current state

The branch already includes the bollard optionality work in `Cargo.toml`,
`src/sandbox/`, `src/orchestrator/`, and
`docs/execplans/feature-gate-bollard.md`.

Two important pieces of evidence have already been gathered:

1. The normal stable no-Docker check currently fails before it reaches the
   branch code:

   ```plaintext
   $ cargo check --no-default-features --features libsql,test-helpers
   error[E0554]: #![feature] may not be used on the stable release channel
   --> .../cap-primitives-3.4.5/src/lib.rs:16:28
   --> .../cap-primitives-3.4.5/src/lib.rs:17:37
   ```

   The log is at:
   `/tmp/check-axinite-feature-gate-bollard-no-docker.out`

2. The dependency path into `cap-primitives` is:

   ```plaintext
   cap-primitives
   └── wasmtime-wasi
       └── ironclaw
   ```

   This was confirmed with:

   ```bash
   cargo tree -i cap-primitives --no-default-features --features libsql,test-helpers
   ```

There is also useful fallback evidence:

1. `RUSTC_BOOTSTRAP=1 cargo check --all-features --features test-helpers`
   finished successfully, proving that the branch does not obviously contain a
   syntax or gross type break in the all-features configuration.
   The log is at:
   `/tmp/check-axinite-feature-gate-bollard-all-features-bootstrap.out`

2. `RUSTC_BOOTSTRAP=1 cargo check --no-default-features --features libsql,test-helpers`
   finished successfully after the no-Docker stub fixes, but emitted warning
   noise from dead code and unused items in the no-Docker configuration.

The clean-build work therefore had two layers:

- restore stable-toolchain compatibility for the `wasmtime-wasi` dependency
  chain, and
- clean up the warning-only fallout in the no-Docker configuration so the
  repository can pass its strict warning-as-error gates.

That work is now complete, and the later wrapper fix has also allowed the
repository-local vendored patch chain to be retired.

## Constraints

- Do not revert or dilute the bollard optionality behaviour already implemented
  on this branch.
- Keep the `docker` feature in `default`.
- Do not switch the repository to a nightly toolchain unless the user gives
  explicit approval. The preferred outcome is a clean stable build.
- Do not add a new external dependency unless there is no practical fix using
  version updates, Cargo patching, or code refactoring.
- Preserve the existing Axinite gate contract: `make check-fmt`,
  `make typecheck`, `make lint`, and `make test` all need tee logs in `/tmp`.
- Do not commit or push until the normal gates are clean. `RUSTC_BOOTSTRAP=1`
  can be used to isolate whether failures are environmental or branch-local,
  but it is diagnostic evidence, not the final gate.

## Tolerances (exception triggers)

- Scope: if fixing the stable-toolchain blocker requires changes outside
  `Cargo.toml`, `Cargo.lock`, the wasm-related dependency surfaces, and the
  no-Docker warning sites in `src/sandbox/` and `src/orchestrator/`, stop and
  escalate.
- Interface: if resolving the warning fallout requires changing public API
  shapes introduced by the bollard plan, stop and escalate.
- Dependencies: if the only viable path is adding a brand-new crate, stop and
  escalate.
- Toolchain: if the only viable path is pinning nightly or changing the repo's
  required Rust toolchain, stop and escalate.
- Iterations: if the same gate fails three times in a row without producing new
  information, stop and document the options.
- Time: if the dependency investigation alone takes more than two hours without
  a concrete remediation candidate, stop and escalate with findings.

## Risks

- Risk: `cap-primitives` may be effectively pinned by the current
  `wasmtime-wasi` version, making a clean stable fix larger than a simple
  patch-level update.
  Severity: high
  Likelihood: medium
  Mitigation: inspect the full `wasmtime` family constraints first, then test a
  minimal version bump or Cargo patch in isolation before touching source code.

- Risk: fixing the stable dependency issue may expose additional branch-local
  warnings or clippy findings that were previously hidden.
  Severity: medium
  Likelihood: high
  Mitigation: rerun the narrow checks first, then the full gate stack, and fix
  findings in smallest-first order.

- Risk: the no-Docker warnings may tempt a broad refactor into separate
  configuration (`cfg`)
  modules, which could accidentally expand scope.
  Severity: medium
  Likelihood: medium
  Mitigation: prefer local `#[cfg]` pruning or helper extraction. If
  suppression is truly unavoidable, do not use `#[allow(dead_code)]`; use a
  tightly scoped `#[expect(dead_code, reason = "...")]` with the reason filled
  in.

## Execution outline

### Milestone 1: Reproduce and pin down the stable-toolchain blocker

Start by rerunning the failing narrow stable check and confirm the error still
originates in `cap-primitives`:

```bash
set -o pipefail
cargo check --no-default-features --features libsql,test-helpers \
  2>&1 | tee /tmp/check-axinite-feature-gate-bollard-no-docker.out
```

Then capture the exact dependency path and versions:

```bash
cargo tree -i cap-primitives --no-default-features --features libsql,test-helpers
cargo tree -i wasmtime-wasi --no-default-features --features libsql,test-helpers
```

The goal of this milestone is to answer one question precisely: is the stable
failure best fixed by updating the `wasmtime` family, by patching one of the
`cap-*` crates, or by some repository-local configuration mismatch?

Document the answer in `Decision Log` before editing code.

### Milestone 2: Apply the smallest stable-compatible dependency fix

Once the dependency root cause is known, prefer the smallest change that makes
the normal stable check pass:

1. Try a constrained dependency update in `Cargo.toml` if the compatible
   version is already within the same intended major line.
2. If that is insufficient, use a targeted Cargo patch or lockfile update.
3. Avoid repository-wide toolchain changes unless the user explicitly approves
   that direction.

After each dependency change, rerun only the narrow stable check until it
passes cleanly.

### Milestone 3: Remove no-Docker warning fallout

After the stable dependency blocker is gone, rerun the narrow no-Docker build.
Use the existing bootstrap evidence as a checklist of likely cleanup sites:

- `src/orchestrator/job_manager.rs`
- `src/orchestrator/reaper.rs`
- `src/sandbox/container.rs`
- `src/sandbox/detect.rs`

The earlier bootstrap run showed warning-only issues such as:

- `validate_bind_mount_path` unused in the no-Docker build,
- `config`, `docker`, and `job_manager` fields unused in no-Docker builds,
- helper functions like `append_with_limit` and Unix socket candidate helpers
  unused when Docker is disabled.

Fix these narrowly. Good fixes include:

- moving Docker-only helpers behind `#[cfg(feature = "docker")]`,
- splitting Docker-only impl blocks from shared structs,
- reducing stored fields in no-Docker configurations only where that does not
  change the public API,
- using tightly scoped `#[expect(dead_code, reason = "...")]` only when an item
  must remain present for interface symmetry and configuration (`cfg`) pruning
  or helper extraction cannot remove the dead path. `#[allow(dead_code)]` is
  not an approved option.

Do not blanket-silence warnings across whole files.

### Milestone 4: Replay the gate stack in increasing cost order

Once the narrow stable no-Docker build is clean, run the branch gates in this
order with tee logs:

```bash
set -o pipefail; make check-fmt 2>&1 | tee /tmp/check-fmt-axinite-feature-gate-bollard.out
set -o pipefail; make typecheck 2>&1 | tee /tmp/typecheck-axinite-feature-gate-bollard.out
set -o pipefail; make lint 2>&1 | tee /tmp/lint-axinite-feature-gate-bollard.out
set -o pipefail; make test 2>&1 | tee /tmp/test-axinite-feature-gate-bollard.out
```

If any gate fails, fix the smallest real issue and rerun from the failing gate
forward. Only rerun earlier gates when a fix plausibly affects them.

### Milestone 5: Final review, commit, and push

After the gates pass:

1. Review the final diff with `git diff --stat` and a targeted `git diff`.
2. Update both execplans to reflect the final clean-build outcome:
   - `docs/execplans/feature-gate-bollard.md`
   - `docs/execplans/feature-gate-bollard-clean-build.md`
3. Commit with a file-based commit message.
4. Push and report any URL returned by the remote.

## Validation and expected evidence

The branch is considered clean only when all of the following are true:

- `cargo check --no-default-features --features libsql,test-helpers` exits 0 on
  the normal stable toolchain.
- `make check-fmt`, `make typecheck`, `make lint`, and `make test` each exit 0.
- The tee logs exist in `/tmp` and do not contain unresolved failures.
- `git status --short` is clean after commit.

Useful transcripts to capture in the final update:

```plaintext
$ cargo check --no-default-features --features libsql,test-helpers
Finished `dev` profile ...

$ make lint
...
Finished ...
```

```plaintext
$ git push
<remote output, including any URL if present>
```

## Progress

- [x] 2026-03-21 00:00 Draft the sibling clean-build execplan.
- [x] Reproduce the stable-toolchain blocker and document the exact cause.
- [x] Choose and apply the smallest stable-compatible fix candidate.
- [x] Remove no-Docker warning fallout in the branch-local code.
- [x] Replay `make check-fmt`, `make typecheck`, `make lint`, and `make test`.
  All four gates now pass on the branch. Evidence logs:
  `/tmp/check-fmt-axinite-feature-gate-bollard.out`,
  `/tmp/typecheck-axinite-feature-gate-bollard-rerun.out`,
  `/tmp/lint-axinite-feature-gate-bollard-rerun.out`, and
  `/tmp/test-axinite-feature-gate-bollard.out`.
- [x] Commit and push with clean gate evidence.
  Final publication was completed by Codex on 2026-03-22, after the clean
  build had already been retired and the later review-fix follow-up had been
  published.
- [x] Fix the ambient `notdeadyet` wrapper so stdin-backed compiler probes are
  stable-safe.
- [x] Prove the unpatched graph in a scratch copy with
  `cargo check --no-default-features --features libsql,test-helpers`.
- [x] Retire the repository-local `[patch.crates-io]` override and the vendored
  `third-party-patches/` carry path.

## Surprises & Discoveries

- 2026-03-21: The first blocker is not the new Docker optionality code. The
  narrow stable no-Docker build fails in `cap-primitives 3.4.5`, reached via
  `wasmtime-wasi`.
- 2026-03-21: The all-features bootstrap build succeeds, which strongly
  suggests the branch is structurally sound and the remaining work is build
  hygiene plus stable-toolchain compatibility.
- 2026-03-21: The no-Docker bootstrap build succeeds but still exposes warning
  cleanup work in the stubbed Docker-off configuration.
- 2026-03-21: `cap-primitives` is not the only crate with this pattern.
  `io-extras 0.18.4` also enables nightly-only `#![feature(...)]` gates behind
  build-script probes.
- 2026-03-21: The ambient `RUSTC_WRAPPER` in this environment returns success
  for an unstable-feature probe that direct `rustc` correctly rejects on
  stable. That makes dependency build scripts mis-detect nightly support.
- 2026-03-21: Merely changing the wrapper configuration is not enough for
  validation. These build scripts only rerun when their own inputs change, so
  stale target artifacts can preserve the bad cfg state until the affected
  crates are cleaned and rebuilt.
- 2026-03-21: Cargo config was not sufficient to override the ambient
  `RUSTC_WRAPPER` for plain `cargo` commands in this environment. Command-line
  `--config` worked, but repository config did not beat the shell environment.
- 2026-03-21: The stable fix therefore moved into vendored crates under
  `third-party-patches/`. Patching only `io-extras` and `cap-primitives` was
  not enough; the same probe bug also affected `cap-std` and
  `system-interface`.
- 2026-03-21: The typecheck rerun surfaced two branch-local test regressions:
  `validate_bind_mount_path` was hidden from tests, and `HashMap` was removed
  from `reaper` test scope. Those are now fixed in-tree.
- 2026-03-21: The first `make lint` rerun after the stable-build recovery did
  not expose new dependency issues. It narrowed the remaining failure to two
  `clippy::clone_on_copy` findings in `src/sandbox/manager.rs`, both caused by
  the no-Docker stub making `DockerConnection` `Copy`.
- 2026-03-21: The lint-only blocker was local to the feature-gating code, not
  the vendored patch chain. Once the clone sites in `SandboxManager` were split
  by cfg, the entire `make lint` matrix completed cleanly.
- 2026-03-21: `make test` validated the vendored stable-build workaround under
  the full test feature set, not just the narrow no-Docker check. The branch
  passed the WebAssembly (WASM) prebuild, `cargo nextest run --workspace --features
  test-helpers`, and `cargo test --manifest-path tools-src/github/Cargo.toml`
  without further source changes.
- 2026-03-21: Fixing `~/.local/bin/notdeadyet` to keep `rustc` in the
  foreground and materialize stdin-backed probes to a temporary file preserved
  heartbeat output while removing the wrapper behaviour that had tainted the
  capability probes.
- 2026-03-21: After the wrapper fix, a scratch copy of the repository with the
  `[patch.crates-io]` stanza removed passed
  `cargo check --no-default-features --features libsql,test-helpers` on the
  stable toolchain. That made the vendored patch chain unnecessary.

## Decision Log

- 2026-03-21: Create a sibling execplan rather than overloading
  `docs/execplans/feature-gate-bollard.md`.
  Reason: the existing file already describes the bollard feature-gating work
  itself. The clean-build recovery work is a separate follow-on stream with its
  own root cause analysis and validation loop.

- 2026-03-21: Treat the stable `cap-primitives` failure as the first problem to
  solve, ahead of the warning-only no-Docker cleanup.
  Reason: until the normal stable toolchain can compile the dependency graph,
  the repository's real gate sequence cannot pass, and warning cleanup alone
  does not make the branch shippable.
- 2026-03-21: Retire the vendored `cap-*` patch chain once the fixed ambient
  wrapper proves the unpatched stable no-Docker build in a scratch copy.
  Reason: the repository should not keep carrying a vendor delta after the
  underlying probe bug has been eliminated from the execution environment.

- 2026-03-21: Keep `RUSTC_BOOTSTRAP=1` as a diagnostic tool only.
  Reason: it is useful for separating branch-local compile problems from
  upstream toolchain or dependency breakage, but it does not satisfy the gate
  contract for this repository.

- 2026-03-21: Treat the stable blocker as a repository-local build-environment
  mismatch, not as a required `wasmtime` upgrade.
  Reason: direct probe reproduction shows that the configured heartbeat
  `RUSTC_WRAPPER` falsely reports unstable std features as supported on stable.
  That explains the failure family across `cap-primitives` and `io-extras`
  without forcing a larger dependency update.

- 2026-03-21: Apply the first fix in `.cargo/config.toml` by forcing
  `RUSTC_WRAPPER` off for this workspace and disabling Cargo's wrapper usage.
  Reason: this is the smallest branch-local change that addresses the actual
  probe input seen by the failing build scripts while preserving the existing
  dependency graph.

- 2026-03-21: Abandon the Cargo-wrapper-config approach in favour of vendored
  crate patches.
  Reason: plain `cargo check` still inherited the ambient shell
  `RUSTC_WRAPPER`, so the repository config was not a durable fix for the
  acceptance command. Patching the affected build scripts to probe `RUSTC`
  directly makes the branch self-contained and keeps plain Cargo working.

- 2026-03-21: Vendor and patch the full failing probe chain now used by the
  branch: `io-extras 0.18.4`, `cap-primitives 3.4.5`, `cap-std 3.4.5`, and
  `system-interface 0.27.3`.
  Reason: fixing the first two crates only moved the failure to the next
  `cap-std` family member. The whole chain shares the same broken probe
  pattern, so patching the full set is the smallest durable stable fix.

- 2026-03-21: Remove the no-Docker warnings by cfg-pruning Docker-only fields
  and helpers rather than adding broad `#[allow(dead_code)]` suppressions.
  Reason: the warnings came from configuration-specific dead paths introduced by
  the optional `docker` feature, so the clean fix is to compile those items only
  when they are genuinely reachable.

- 2026-03-21: Fix the remaining `clippy::clone_on_copy` findings with
  cfg-specific locals at the clone sites instead of weakening lints or changing
  the shared `DockerConnection` alias shape.
  Reason: the Docker-enabled build still needs an owned client handle, while
  the Docker-disabled stub is intentionally `Copy`. Splitting behaviour at the
  use sites keeps both configurations honest without distorting the shared type
  surface.

## Outcomes & Retrospective

The stable-toolchain blocker was caused by build-script feature probes trusting
an ambient `RUSTC_WRAPPER` that falsely reported unstable support on stable.
The durable branch-local fix was to vendor and patch the affected probe chain
so it invokes `RUSTC` directly during capability detection.

The no-Docker warning fallout was resolved with narrow cfg-gating in the
sandbox and orchestrator modules, plus one cfg-specific `clone_on_copy` fix in
`src/sandbox/manager.rs`. No public API expansion or toolchain switch was
required.

The branch now satisfies the clean-build goal:

- `cargo check --no-default-features --features libsql,test-helpers` passes on
  stable,
- `make check-fmt`, `make typecheck`, `make lint`, and `make test` all pass,
- the acceptance logs exist under `/tmp`.

Commit and push were completed by Codex on 2026-03-22. The plan now remains as
retired implementation history rather than outstanding work.
