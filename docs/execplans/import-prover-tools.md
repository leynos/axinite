# Import Kani and Verus support from Netsuke

Status: DRAFT — awaiting approval before implementation.

## Purpose and big picture

Axinite needs reproducible proof tooling before features such as the Monty
helper can discharge their verification obligations. This plan imports the
reusable Kani and Verus integration from Netsuke, adapts it to Axinite's build
policy, and demonstrates both tools with small, executable examples.

Kani is a bounded model checker: it explores all reachable executions within
explicit limits. Verus is a deductive verifier: it checks mathematical
contracts and inductive proofs over Rust-like source. Neither tool establishes
that an operating system, subprocess, or asynchronous protocol behaves
correctly merely because a model passes.

Success means a contributor can install pinned tools, run `make kani`,
`make kani-full`, and `make verus`, and observe named checks succeed.
Deliberate faults must make those same commands fail. Continuous integration
(CI) must exercise the checked-in examples and report tool versions and proof
results. Ordinary application builds must remain independent of installed
provers.

This is a pre-implementation plan. This branch does not install tools, add
proofs, change runtime behaviour, or complete roadmap task 2.2.1.

## Conformance basis

The inspected Axinite baseline is `228bd47ed21e13d8c8cf15375d348b3542ac65a3`.
Read `AGENTS.md`, `docs/contents.md`, `docs/welcome-to-axinite.md`,
`docs/axinite-architecture-overview.md`, `docs/repository-layout.md`, and
`docs/documentation-style-guide.md` before execution.

The following requirements provide the architecture contract. There is no
separate approved terms-of-reference document for this tooling import.

- R1: Import reusable Netsuke support, using the source inventory below;
  discharge through M1 and M2 installation and command-contract evidence.
- R2: Preserve Axinite's development build standard and runtime boundaries;
  discharge through M2 flag-contract tests and M4 ordinary repository gates.
- R3: Prove the tooling actually checks substantive obligations and propagates
  failure; discharge through M3 positive examples and negative controls.
- R4: Provide reproducible local and CI execution, documented limits, and a
  usable hand-off to Monty; discharge through M4 workflow evidence and guides.

`docs/formal-verification-methods-in-axinite.md`, especially its tooling and CI
sections, recommends pinned Kani/Verus support outside production Cargo. Its
shell installer examples are recommendations, not existing implementation. The
delegated package described below replaces those example installers.

The [Monty helper ExecPlan][monty-plan], inspected on 2026-10-28, proposes
`crates/monty-capabilities` and `verus/monty_capabilities.rs`. It requires
exact projection of permitted numeric tool identities, a production compilation
bridge, and rejection of stale generated artefacts. This import enables that
work but does not implement or weaken those requirements. Its M1 must still
select and demonstrate a shared-source or deterministic proof-erasure bridge; a
separate proof model and sampled tests alone do not meet that contract.

[monty-plan]: https://github.com/leynos/axinite/blob/2-2-1-helper-subprocess-wrapper-for-monty/docs/execplans/2-2-1-helper-subprocess-wrapper-for-monty.md

## Context and source inventory

Axinite currently has no Kani or Verus targets in `Makefile` and no prover pins
under `tools/`. `tools/mold/` already holds build-tool pins.
`tests/workflow_contracts/` contains Python tests of repository-owned build and
CI contracts; `make test-workflow-contracts` runs them. The root
`rust-toolchain.toml` pins `nightly-2026-10-16`, while `.cargo/config.toml`
selects `-Zthreads=8` and Linux `mold` linking. These settings cannot blindly
flow into verifier-managed compilers.

Use Netsuke revision `e52d3ef49429392eebeb484aa4d6622ca79782dd`, resolved from
GitHub on 2026-10-28, as the reviewable donor snapshot. Obtain files using
`git show <revision>:<path>` from a fetched read-only checkout or GitHub's
contents API with that exact revision. Do not copy an arbitrary local HEAD. The
relevant source paths are:

- `Makefile`: `PROVER_TOOLS_SOURCE`, `PROVER_TOOLS`, Kani command/version
  settings, install/check/run targets, diagnostic and exit-status handling, and
  the verifier-specific `KANI_RUSTFLAGS` boundary.
- `tools/kani/VERSION`: Kani `0.67.0`.
- `tools/verus/VERSION` and `tools/verus/SHA256SUMS`: Verus
  `0.2026.10.04.426d8b0`, with an `x86-linux` archive digest.
- `verus/canonical_value/main.rs`: a non-vacuous proof with explicit limits
  on its relationship to runtime code. Use it as a review example, not an
  Axinite domain implementation.
- `.github/workflows/ci.yml`, `scripts/ci/install_kani.py`, and
  `.github/actions/kani-cache/action.yml`: examples of pinned archive
  installation, cache ownership, time bounds, and proof execution.
- `tools/kani/proof-scope.toml`, `scripts/kani_proof_scope.py`, and
  `tests/workflow_contracts/kani_*`: examples of tested input selection and
  failure reporting. Do not import Netsuke's graph-specific selection rules,
  telemetry, systemd scope wrapper, or mutation corpus.

Netsuke delegates its local install and Verus run commands to
`git+https://github.com/leynos/rust-prover-tools@2ba17da6b7e24160c4041e8d41ba1ba6cca87b35`.
At that revision, `docs/users-guide.md` specifies these interfaces:

```bash
prover-tools kani install
prover-tools kani check-version --version-file tools/kani/VERSION
prover-tools verus install
prover-tools verus run --proof-file verus/tooling_smoke/main.rs
```

The Verus installer verifies the archive digest before extraction. Its runner
can install the toolchain reported by Verus. Its default proof path is a legacy
example, so Axinite must always supply an explicit proof path. An invalid
explicit binary override can fall back to the default installation: tests and
documentation must not misrepresent that behaviour as strict selection.

Netsuke's `formal-pr` only checks the Kani version. Its canonical-value model
is not a mechanically verified compilation of the production implementation.
Neither contract is sufficient evidence for Axinite's R3 or Monty's runtime
proof bridge. Preserve provenance and applicable licence notices for copied
material; compare donor and recipient licences before copying source.

## Constraints

Implement only after explicit approval. Keep the tools outside runtime
manifests and application startup. No new runtime port or service is needed:
Make and CI invoke development tools, while domain proof examples remain pure.
Do not restructure Axinite into a new directory hierarchy for this import.

Use the shared default Cargo cache locally. Never unset or override managed
`RUSTC_WRAPPER` or `RUSTDOC`, bypass compiler admission, or use `/tmp` as a
build target. Dedicated target output below the worktree is acceptable;
isolated Cargo homes on this shared host are not. Do not transplant Netsuke's
CI home redirections into local recipes.

Keep the existing nightly, `mold`, release, coverage, and Whitaker policies.
Verifier commands must select their supported flags explicitly and test the
boundary, including inherited `RUSTFLAGS` and `CARGO_ENCODED_RUSTFLAGS`.
Incompatible caller flags must fail with a useful diagnostic or be excluded by
a documented, command-local policy; never silently mutate global settings.

Do not import Netsuke's application harnesses. Do not claim a sample proves
Monty, tool authorization, process cleanup, or concurrent protocol safety. Do
not add Stateright, Loom, or other verification frameworks in this task. Keep
`make all` independent of downloaded provers; add explicit formal targets. Check
`FEATURE_PARITY.md`, but change statuses only for actual delivered features,
not for prospective proof coverage.

## Tolerances (exception triggers)

Stop and record a proposed deviation for approval if the work needs a runtime
API change, a production crate extraction, a root toolchain upgrade, a managed
compiler-wrapper change, or weakened proof obligations. Also stop if the pinned
package cannot support the required invocation safely without changes in its
own repository. Propose that upstream fix separately.

The implementation budget is at most 20 non-ExecPlan files and 1,500 added or
changed lines, excluding this and all other ExecPlan files from both counts. Do
not spend more than two bounded compatibility experiments on the same compiler
incompatibility without recording evidence and escalating. A healthy
compiler-admission queue is not a failed experiment or a time-limit breach.
Initial supported execution is Linux x86-64. Additional archive targets require
verified digests and an explicit plan revision. CI jobs have a provisional
30-minute timeout; adjust only with measured installation and proof costs.

## Risks

Kani's bundled compiler may not accept Axinite's dependency graph. M1 therefore
uses a tiny standalone fixture outside the application workspace; verifying
arbitrary production crates is explicitly a later consumer decision.

Verus's proof syntax is not ordinary Rust, and a model can drift from
executable code. The initial example is labelled tooling validation. Monty must
retain its own stronger compilation/refinement acceptance criterion.

Archive trust, transient downloads, and stale toolchain caches can produce
misleading results. Pin package revisions and archive digests, check versions,
record cold and repeat runs, and start without a new custom CI cache. A cache
optimization is unnecessary for initial correctness.

The repository's formal-methods recommendations describe broader adoption than
this task. Update those recommendations to distinguish available tooling,
demonstrated examples, and future domain proofs without marking the whole
adoption programme complete.

## Progress

- [x] (2026-10-28) Read recipient policies and the Monty proof requirements.
- [x] (2026-10-28) Resolve live Netsuke source and inspect delegated CLI
      contracts.
- [x] (2026-10-28) Draft the import and verification milestones.
- [ ] Obtain explicit implementation approval.
- [ ] M1: Establish pinned compatibility and provenance.
- [ ] M2: Add tested local command boundaries.
- [ ] M3: Demonstrate both tools and reject deliberate faults.
- [ ] M4: Integrate CI, documentation, and final validation.

## Surprises & discoveries

The local Netsuke default ref was older than the live donor revision. The older
snapshot lacked Verus pins and proofs; the live revision includes them and
updates the shared tooling pin. The source inventory above uses the live
revision, not the stale snapshot.

The delegated Verus runner has a legacy default proof path and fallback binary
selection. Explicit Axinite entry points and observable version output are
necessary. Netsuke's version-only aggregate must not be described as proof
execution when adapted.

## Decision log

- (2026-10-28) Reuse the pinned shared package rather than fork installer
  scripts. This follows current Netsuke practice and centralizes download and
  verifier-toolchain handling.
- (2026-10-28) Use isolated source fixtures, not an isolated Cargo cache, for
  initial executable evidence. This contains compiler compatibility work
  without promising whole-application model checking.
- (2026-10-28) Start with explicit, uncached CI proof jobs. Netsuke's cache,
  telemetry, and change-selection system is substantially larger than the
  tooling import and is unnecessary for the first small harness set.
- (2026-10-28) Keep the Monty runtime bridge in its owning feature plan. This
  import must supply usable proof commands and document the remaining gap.

## Plan of work

### M1 — Pin and demonstrate compatibility

R1 and R2 apply. Re-read the exact donor files and package CLI documentation.
Record provenance, licence compatibility, versions, digests, toolchain
requirements, and supported architecture in this plan. Confirm the package's
Python requirement against Axinite's `uv` invocation before choosing flags.

In disposable source scratch below the worktree's ignored `target/`, run a
minimal Kani assertion and a minimal Verus lemma through the pinned package.
Use the existing Cargo home and managed compiler admission. Record the exact
commands, selected compilers, wrapper behaviour, and exit codes. Probe both a
true assertion and an intentionally false assertion; a tool that only prints
its version does not satisfy the experiment.

The plateau is a documented compatibility decision with no application change.
Acceptance is real successful verification plus rejected false assertions for
both tools. Missing tools or a refused compiler invocation are inconclusive,
not proof failures. If compatibility fails, preserve logs, mark the plan
BLOCKED, and propose a narrower supported boundary. Remove only owned scratch
sources on recovery; never delete shared caches. M2–M4 remain outstanding.

### M2 — Add local installation and execution contracts

R1 and R2 apply. Add the three pin/checksum files, adapt the shared package
variables and local targets in `Makefile`, and ignore `.verus/` installation
output. Explicitly pass version-file overrides to the CLI; merely echoing a
Make variable does not make the installer honour it.

Add `tests/workflow_contracts/prover_tools_test.py` first. Use temporary fake
executables to record arguments, environment, selected proof files, and exit
codes. Exercise missing/malformed pins, version mismatch, quoted command
arguments, custom pin paths, non-zero verifier exits, and sensitive flag
redaction. Run `make test-workflow-contracts` and capture the expected red
failure before editing recipes. Implement the smallest recipes that pass.

Provide `install-kani`, `kani-check`, `kani`, `kani-full`, `install-verus`,
`verus`, and `formal-pr`. `kani` selects the smoke harness explicitly;
`kani-full` runs every harness in the checked-in verification fixture;
`formal-pr` runs `kani` followed by `verus` sequentially. Both Kani run targets
must check the pin first. No Netsuke-specific `kani-ir` alias is needed.

Set an explicit default Verus entry point and provide an override, for example
`VERUS_PROOF_FILE=verus/monty_capabilities.rs`. Test that the override reaches
`--proof-file` exactly once. Test verifier flag isolation from the ordinary
Cargo configuration without removing managed wrappers. Retain useful source,
version, and target diagnostics without logging arbitrary secret-bearing flags.

M2's plateau exposes installation/version checks and execution contracts; until
M3 adds fixtures, proof run targets must fail clearly on missing input.
Acceptance is passing command-contract tests with non-zero failures preserved,
plus successful pinned installation from M1. Check R1/R2 conformance before
committing. Recover by reverting only M2's uncommitted changes; application
builds remain unchanged. Actual checked-in proof evidence remains for M3.

### M3 — Add non-vacuous verification examples

R3 applies. Add `tests/verification/kani-smoke/Cargo.toml` as a standalone
package with its own workspace boundary and explicit Rust edition compatible
with Kani. Add `src/lib.rs` beneath that directory. Keep its target output
under the worktree and its package out of the main application's dependencies.
Use `#[cfg(kani)]` harnesses and appropriate `check-cfg` declarations.

The example computes a four-entry capability intersection as a bit mask. For
arbitrary `u8` requested and permitted masks, the result must contain exactly
the low four bits present in both inputs. Specify membership per bit
independently of the implementation's bitwise expression. Include empty,
disjoint, and overlapping witnesses. Use explicit unwind bounds sufficient for
four membership checks and retain unwinding assertions. Name the harness
`verify_exact_projection_four_entries` and select it in `make kani`.

Add `verus/tooling_smoke/main.rs` proving that mathematical set intersection
contains an element if and only if both input sets contain it. Prove both
inclusion directions and exhibit empty and non-empty instances. This is an
unbounded specification example, not a proof that the Kani fixture or any
production function implements it. No `assume`, `admit`, or trusted external
body may establish the result.

First capture red evidence with a faulty union implementation in the Kani
fixture and a false Verus equality. Correct each and capture green evidence.
Keep reproducible negative controls in
`tests/workflow_contracts/prover_negative_controls_test.py`, using temporary
copies so the passing sources are never mutated in place. The negative tests
must distinguish the intended assertion failure from syntax errors, missing
binaries, and timeouts. Expose these real-tool tests as
`make test-prover-tools`; ordinary contract tests must not download provers.

The plateau is `make formal-pr` executing both genuine checks and
`make test-prover-tools` showing both deliberate faults rejected. Review each
precondition, witness, bound, and trust assumption. Recovery restores the last
committed passing fixtures; no generated proof output is a source of truth. CI
and contributor documentation remain for M4.

### M4 — Publish CI and the consumer hand-off

R2 and R4 apply. Add `.github/workflows/formal-verification.yml` with separate
`kani-smoke` and `verus-proofs` jobs on Linux x86-64, using pinned actions,
read-only contents permissions, and the same Make targets as local runs.
Trigger on pull requests, main-branch pushes, a weekly schedule, and manual
dispatch. Keep the initial examples small enough to run on every trigger; do
not introduce path-selection logic in this import. Do not use privileged
pull-request triggers or repository secrets to execute contributor code.

Install each pinned tool in its job, run its positive and corresponding
negative controls, and retain concise version/results logs on failure. Use
30-minute job bounds initially. Add workflow-contract tests before workflow
implementation, covering events, permissions, pins, timeouts, and commands. Run
those tests red, implement, then run green. Do not copy Netsuke's host-specific
runner selection or change repository branch protection here.

Update `docs/developers-guide.md`,
`docs/formal-verification-methods-in-axinite.md`, and `docs/contents.md` with
installation, exact target semantics, compiler boundaries, expected failures,
and instructions for adding a harness or Verus entry point. Review
`docs/repository-layout.md` and `FEATURE_PARITY.md` for necessary factual
updates. State explicitly that the sample is not Monty's production proof.
Explain how the Monty feature supplies its own file and what bridge evidence it
still owes. Do not complete roadmap task 2.2.1.

The final plateau is successful local gates and successful hosted proof jobs at
the candidate commit, including a manual repeat run. Record elapsed times,
source revision, versions, named assertions, and negative-control outcomes.
Check all R1–R4 links and any upstream documentation deviations before marking
this plan COMPLETE. Recover failed CI by preserving the run/log links and
fixing the smallest demonstrated defect, without weakening assertions.

## Verification plan

V1, for R1/M1–M2: configured pins and proof paths determine the invoked tools
and inputs, and any failed check fails the target. Fake-executable contract
tests cover argument forwarding and exit propagation; actual installs and
version probes exercise the external boundary. Missing pins, mismatched
versions, and a stub exiting 23 are negative controls. Discharge requires
expected non-zero target exits and real matching installed versions, not string
matching against an echoed pin.

V2, for R2/M2–M4: ordinary builds retain their existing compiler flags and
provers do not enter runtime dependencies. Contract tests compare ordinary Make
invocations with verifier invocations and exercise both Rust flag environment
forms. `make all` and manifest inspection provide integration evidence. A
deliberately leaked `-Zthreads=8` in a fake verifier invocation must fail its
contract test. The compiler and package manager remain trusted external
interfaces; their internal correctness is not re-proved.

V3, for R3/M3: the four-entry projection retains exactly authorized requested
entries. The independent per-index membership specification is the connecting
lemma. Kani explores arbitrary input masks with four reachable membership
checks and an adequate explicit unwind bound. Empty, disjoint, overlapping, and
high-bit inputs demonstrate reachable partitions. Replacing intersection with
union must yield a property counterexample. The guarantee is restricted to this
fixture and four entries; it says nothing about production dispatch.

V4, for R3/M3: Verus establishes both directions of intersection membership for
arbitrary mathematical sets. The two inclusion lemmas establish equality; no
assumption may restate that equality. Empty and singleton witnesses show
satisfiable inputs. A union-equals-intersection negative control must fail
verification, while the valid proof reports at least one verified obligation
and zero errors. Trust includes Verus, its standard specification library, and
its solver; there is no claimed executable refinement in this example.

V5, for R4/M4: CI executes the same named positive and negative checks at the
candidate revision and reports failures. Workflow tests establish static
wiring; successful hosted jobs and retained negative-control results establish
execution. A workflow omitting a run step must fail the static test. Hosted
runner/network availability is an operational assumption, and timeout or
installation failure is never classified as a successful negative control.

## Concrete validation and recovery

Run commands from the repository root, one gate at a time, with `pipefail` and
logs under `/tmp`. During implementation the relevant sequence is:

```bash
set -o pipefail
make test-workflow-contracts 2>&1 | tee /tmp/prover-contracts-axinite.out
make install-kani 2>&1 | tee /tmp/prover-install-kani-axinite.out
make install-verus 2>&1 | tee /tmp/prover-install-verus-axinite.out
make formal-pr 2>&1 | tee /tmp/prover-formal-pr-axinite.out
make kani-full 2>&1 | tee /tmp/prover-kani-full-axinite.out
make test-prover-tools 2>&1 | tee /tmp/prover-negative-controls-axinite.out
make fmt 2>&1 | tee /tmp/prover-fmt-axinite.out
make all 2>&1 | tee /tmp/prover-all-axinite.out
make markdownlint 2>&1 | tee /tmp/prover-markdownlint-axinite.out
make nixie 2>&1 | tee /tmp/prover-nixie-axinite.out
git diff --check
```

Run `make audit` when manifests, lockfiles, or CI supply-chain policy change. A
successful negative-control suite exits zero only after asserting that each bad
proof failed for the intended reason. The underlying bad proof commands must
exit non-zero. Record actual transcripts rather than predicted counts. Each
milestone commit needs the applicable repository gates; full code changes need
`make all`. Re-run gates after fixes, never overlap them for this candidate.

For this plan-only review, run `make check-fmt`, `make markdownlint`,
`make nixie`, and `git diff --check`. Runtime proof or Rust test execution is
not evidence required for prose that describes future work. Do not report those
future commands as already passed.

Installations and proof runs must be safely repeatable. Retry a failed download
through the pinned installer; never accept a checksum mismatch or remove the
pin to get past it. Do not remove shared toolchains or caches. Retain failures
and exact commands for the next owner. On a tolerance breach, update the
Decision log and status to BLOCKED and await approval of the proposed change.

## Outcomes & retrospective

The plan defines a bounded import with pinned provenance, executable positive
and negative verification evidence, CI integration, and explicit limits on what
the examples prove. Implementation has not started. Future milestones must
replace expectations with actual commands and results. CodeGraph indexing timed
out during planning; the source inventory was checked with direct reads at the
pinned revisions.

## Revision notes

- (2026-10-28) Initial draft based on the live Netsuke source, the delegated
  package contract, Axinite's build standard, and the Monty proof obligations.
