<!-- markdownlint-disable-next-line MD013 -->
# Architectural decision record (ADR) 007: Stable capability probes must ignore ambient `RUSTC_WRAPPER`

## Status

Accepted. Carry a narrow vendored patch chain for the affected `cap-*`
build-script probes until the dependency graph or build environment provides
an equivalent stable-safe behaviour without repository-local patches.

## Date

2026-03-21

## Context and problem statement

The `feature-gate-bollard` branch makes Docker-backed sandboxing optional at
compile time, which introduces an important no-Docker acceptance path:

```bash
cargo check --no-default-features --features libsql,test-helpers
```

That acceptance command failed on stable before it reached branch code. The
failure came from the `wasmtime-wasi` dependency chain through
`cap-primitives`, `cap-std`, `io-extras`, and `system-interface`. Their build
scripts probe unstable feature support and were treating the ambient
`RUSTC_WRAPPER` as authoritative for those probes. In this environment, the
wrapper returned success for a probe that direct `rustc` correctly rejected on
stable, which caused the crates to enable nightly-only cfg paths and then fail
later in compilation.

The problem is not Docker optionality itself. The problem is that stable
capability detection was coupled to an execution wrapper whose behaviour is not
equivalent to invoking `RUSTC` directly. The repository needs a durable way to
keep stable acceptance commands honest without forcing a nightly toolchain or
depending on ad hoc operator shell state.

## Decision drivers

- Keep stable-toolchain acceptance commands reliable
- Preserve plain `cargo` behaviour, not only bespoke wrapper scripts
- Avoid broad dependency churn unrelated to the Docker feature-gating change
- Minimize the carried delta to narrowly scoped build-script fixes
- Define a retirement path so the branch does not carry vendored patches
  indefinitely

## Requirements

### Functional requirements

- The no-Docker stable acceptance command must succeed without
  `RUSTC_BOOTSTRAP`.
- Default-feature and all-feature gate runs must continue to work with normal
  `cargo` and `make` invocations.
- The fix must not change the runtime behaviour of the affected crates beyond
  correcting their compile-time capability detection.

### Technical requirements

- Build-script probes must use the real compiler capability surface rather than
  wrapper-specific behaviour.
- The repository-local fix must be explicit and reviewable in version control.
- The retirement criteria must be objective enough that a future maintainer can
  remove the patches confidently.

## Options considered

### Option A: Carry a narrow vendored patch chain

Vendor the affected crates under `third-party-patches/` and patch only their
`build.rs` probe logic so it invokes `RUSTC` directly instead of inheriting
probe results from the ambient `RUSTC_WRAPPER`.

This keeps the fix local to the actual fault, works with plain `cargo`, and
avoids changing the repository toolchain contract. The cost is that the
repository carries a temporary vendor delta that must be monitored and later
retired.

### Option B: Upgrade to upstream crate releases that no longer need the patch

Update the `wasmtime-wasi` and `cap-*` family to versions whose build scripts
either probe `RUSTC` directly or otherwise remain stable-safe under the
ambient wrapper.

This is the preferred long-term outcome because it removes the carried patch.
It was not the right immediate fix for this branch because the dependency graph
was already pinned to working runtime versions, and the root-cause evidence
showed a probe bug rather than a known functional defect in the selected
versions.

### Option C: Enforce a wrapper-neutral build environment

Neutralize `RUSTC_WRAPPER` in repository or CI configuration so the build
scripts see direct compiler behaviour even when operators run plain `cargo`
commands.

This avoids vendoring, but it proved unreliable for the acceptance path that
matters here. Repository-local Cargo configuration did not consistently beat
the ambient shell environment for plain `cargo` invocations, and command-line
overrides would not satisfy the requirement that the branch behave correctly
without special operator choreography.

### Option D: Require nightly or `RUSTC_BOOTSTRAP`

Treat the probe outcome as acceptable and make the branch rely on nightly or
bootstrap builds.

This removes the immediate failure, but it weakens the repository's stable
toolchain contract and hides the underlying probe error instead of fixing it.
It also makes the no-Docker acceptance path less trustworthy, so it is not an
acceptable steady-state solution.

## Decision outcome / proposed direction

Choose **Option A** for the current branch.

The repository will carry vendored patches for:

- `io-extras`
- `cap-primitives`
- `cap-std`
- `system-interface`

The carried delta must stay narrow:

- patch only the build-script probe path,
- keep the vendored crate versions aligned with the locked dependency graph,
- avoid source edits outside the capability-detection fix unless a separate
  defect is being addressed with its own review trail.

### Retirement criteria

The vendored patch chain must be removed when any one of the following becomes
true and is validated on this repository:

1. The dependency graph can be updated to upstream releases that no longer rely
   on wrapper-tainted probe results, and the unpatched graph passes:
   - `cargo check --no-default-features --features libsql,test-helpers`
   - `make check-fmt`
   - `make typecheck`
   - `make lint`
   - `make test`
2. The affected crates leave the dependency graph entirely, so the vendored
   patches are no longer referenced.
3. The repository gains a durable, documented, and verified build mechanism
   that makes plain `cargo` invocations wrapper-neutral for capability probes,
   and the unpatched dependency graph passes the same acceptance commands above.

The patches must not be retired based only on a local anecdotal success. The
retirement branch must prove the unpatched path against the full acceptance
commands listed above.

## Goals and non-goals

- Goals:
  - Keep stable builds deterministic for both Docker and no-Docker paths.
  - Limit the carried fix to the actual capability-probe defect.
  - Make eventual patch removal straightforward and auditable.
- Non-goals:
  - Fork or maintain functional changes to the `cap-*` crates.
  - Use vendoring as a general dependency-management strategy.
  - Normalize operator environments around one specific wrapper setup.

## Migration plan

1. Keep the current vendored patch chain in place while the branch depends on
   the affected crate versions.
2. Periodically check whether upstream releases or dependency updates remove the
   need for the patch.
3. When a retirement candidate exists, test the unpatched graph on a dedicated
   branch by removing the relevant `[patch.crates-io]` entries and vendored
   directories.
4. Remove the patch chain only after the unpatched branch passes the stable
   no-Docker acceptance command and the full gate stack.
5. Update this ADR, `docs/contents.md`, and any related implementation notes to
   record the retirement.

## Known risks and limitations

- Vendored dependencies increase repository weight and review noise.
- Upstream security or bug-fix updates still require active monitoring because
  vendored copies do not update themselves.
- The retirement trigger depends on future validation work; it is easy to leave
  temporary patches in place if nobody owns the follow-up.

## Architectural rationale

This decision protects the repository's stable-toolchain contract at the
architectural boundary where it was actually violated: compiler capability
detection. It keeps Docker feature-gating and sandbox behaviour independent
from a separate build-environment fault, and it makes the workaround explicit,
bounded, and removable.

The chosen approach also preserves an important maintainability property:
repository acceptance should depend on declared source and lock state, not on
ambient shell quirks. Carrying a small, well-documented patch is preferable to
depending on invisible wrapper semantics that a future maintainer cannot infer
from the repository alone.

## References

- `docs/execplans/feature-gate-bollard-clean-build.md`
- `Cargo.toml`
