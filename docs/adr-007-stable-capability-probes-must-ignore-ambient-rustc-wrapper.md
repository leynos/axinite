<!-- markdownlint-disable-next-line MD013 -->
# Architectural decision record (ADR) 007: Stable capability probes must ignore ambient `RUSTC_WRAPPER`

## Status

Accepted

## Date

2026-03-21

This ADR records a decision whose interim vendored workaround was later
retired once the ambient wrapper bug was fixed and the unpatched dependency
graph passed the required stable acceptance command.

## Context

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

## Problem statement

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

Choose **Option A** for the initial branch recovery.

The repository originally carried vendored patches for:

- `io-extras`
- `cap-primitives`
- `cap-std`
- `system-interface`

The carried delta was kept narrow:

- patch only the build-script probe path,
- keep the vendored crate versions aligned with the locked dependency graph,
- avoid source edits outside the capability-detection fix unless a separate
  defect is being addressed with its own review trail.

### Retirement criteria

The vendored patch chain had to be removed when any one of the following became
true and was validated on this repository:

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

The patches were not retired based only on anecdotal local success. Retirement
required a verified unpatched build path against the repository acceptance
commands.

### Retirement outcome

On 2026-03-21, the ambient `notdeadyet` wrapper was fixed so stdin-backed
compiler probes no longer depended on a backgrounded `rustc` process. The
retirement proof used a scratch copy of this repository with the
`[patch.crates-io]` stanza removed and reran the original stable no-Docker
acceptance command:

```bash
cargo check --no-default-features --features libsql,test-helpers
```

That unpatched build succeeded on the stable toolchain, which satisfied the
root retirement criterion for the original blocker. The repository then
removed the vendored `cap-*` patch chain and the Dockerfile packaging carry
for `third-party-patches/`.

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
   the affected crate versions and the ambient wrapper still taints probes.
2. Test the unpatched graph on a dedicated scratch tree by removing the
   relevant `[patch.crates-io]` entries and vendored directories.
3. Remove the patch chain only after the unpatched branch passes the stable
   no-Docker acceptance command and the full gate stack.
4. Update this ADR, `docs/contents.md`, and any related implementation notes to
   record the retirement.

## Known risks and limitations

- Ambient compiler wrappers can still distort build-script capability probes if
  they do not preserve stdin semantics faithfully.
- Future wrapper changes should be treated as build-contract changes and
  revalidated against the stable no-Docker acceptance command when they affect
  `rustc` invocation shape.

## Architectural rationale

This decision protected the repository's stable-toolchain contract at the
architectural boundary where it was actually violated: compiler capability
detection. It kept Docker feature-gating and sandbox behaviour independent from
a separate build-environment fault, and it made the workaround explicit,
bounded, and removable.

The retirement also preserves an important maintainability property:
repository acceptance should depend on declared source and lock state, not on
ambient shell quirks. Once the wrapper was fixed, the repository no longer
needed to carry a local vendor delta to defend itself from that environment bug.

## References

- `docs/execplans/feature-gate-bollard-clean-build.md`
- `Cargo.toml`
