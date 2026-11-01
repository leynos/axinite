# Import Kani and Verus support from Netsuke

Status: IN PROGRESS

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

This approved plan authorizes the tooling import below. It does not authorize
implementation of roadmap task 2.2.1 or claim its proof obligations are met.

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

Netsuke's donor snapshot delegates to Rust Prover Tools revision
`2ba17da6b7e24160c4041e8d41ba1ba6cca87b35`. Keep that revision as historical
provenance only. The implementation must instead pin
`git+https://github.com/leynos/rust-prover-tools@592558f7183b554fc749696246984c5d7300d029`,
the upstream `main` tip verified on 2026-11-01. Do not use a floating `main`
requirement. The Netsuke snapshot still supplies repository integration
examples; this newer package supplies the authoritative CLI contract.

Read the pinned [user guide][prover-guide], [migration guide][prover-migration],
`pyproject.toml`, and `rust_prover_tools/binary_assets.py`. Python 3.14 or
newer is required, so use `uv tool run --python 3.14 --from` with the pinned
source. The migration guide is titled 0.2.0 but package metadata still says
0.1.0: identify the consumed contract by commit, not an inferred published
version. Axinite consumes the CLI, not the changed Python `VerusRunOptions`
interface.

The updates relevant to this import are:

- `kani install` now defaults to prebuilt Linux x86-64 Kani 0.67.0. It
  verifies the frontend and bundle digests before extracting the frontend and
  invoking setup with the local bundle. `--compile` is an explicit source-build
  fallback, not an automatic response to download failure. Use `--no-compile`
  to prevent an inherited `INPUT_KANI_COMPILE` from changing the route. Kani
  and Z3 asset pins are owned by the package; do not duplicate its downloader
  or Netsuke's `scripts/ci/install_kani.py` in Axinite.
- `kani exec` runs an explicitly selected, installed Kani 0.67.0 release with
  its `nightly-2025-11-21` toolchain and matching LLVM runtime. It requires
  `--installation`, `--cwd`, and at least one literal `--extra-arg`. It does
  not download tools. Run from the proof package; `--manifest-path`,
  `--config`, and `--profile` arguments are rejected. The frontend install
  directory and the release root passed to `--installation` are different paths.
- The execution boundary preserves configured `RUSTC_WRAPPER`,
  `RUSTC_WORKSPACE_WRAPPER`, and `RUSTDOC`, uses encoded-flag precedence,
  removes incompatible development flags, and applies LLVM profile overrides
  only to its child. Unsupported or ambiguous routing fails before compilation.
  Bare coverage instrumentation flags are removed for the documented ambient
  flag mode; a wrapper that injects coverage later remains unqualified.
- `verus run` accepts repeated `--proof-file` arguments, validates every path
  before starting, runs them in order, and stops at the first failure. It names
  the failing file and previously proved files. Environment proof-file bindings
  still select one file; explicit CLI paths override them. The legacy default
  path still exists, and invalid explicit binary overrides still fall back:
  always supply proof paths and record the actual selected binary/version.
- Verus streaming preserves the prover's exit status even when stdout or
  stderr closes early. A shell pipeline can still hide it; capture
  `PIPESTATUS[0]` immediately when testing this boundary, independently of later
  `tee` or `head` statuses.
- `z3 install` installs package-pinned Z3 4.16.0 for Linux x86-64, with a
  verified archive and a version marker for reuse. Pass its absolute binary
  path via `VERUS_Z3_PATH`; do not assume a Verus archive supplies the solver.
  M1 must establish compatibility with the retained Verus release pin.
- All three archive installers use bounded direct HTTPS downloads, verify
  digests, and limit transfer time and size. Proxy environment variables are
  not used. Download and cleanup failures remain failures; installer debug
  events are opt-in and do not require new Axinite telemetry.

The key upstream changes are #195, #198, and #217 for Kani execution, #220 for
multi-file Verus runs, #222 for exit-status preservation, and #226 for binary
installers. Source inspection establishes the available interfaces, not that
Axinite has already exercised them.

[prover-guide]: https://github.com/leynos/rust-prover-tools/blob/592558f7183b554fc749696246984c5d7300d029/docs/users-guide.md
[prover-migration]: https://github.com/leynos/rust-prover-tools/blob/592558f7183b554fc749696246984c5d7300d029/docs/v0-2-0-migration-guide.md

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
Delegate Kani compiler selection and flag normalization to `kani exec`, then
test Axinite's invocation against that boundary, including inherited
`RUSTFLAGS` and `CARGO_ENCODED_RUSTFLAGS`. Do not recreate the package's
normalizer or remove wrappers in Make recipes. Unsupported routing must fail
with a useful diagnostic; never silently mutate global settings. Do not add
nested coverage execution in this import or claim its wrapper mode is supported.

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

The new Z3 asset targets `x64-glibc-2.39`; Linux x86-64 alone does not
guarantee that it runs on every host. M1 must check the host ABI and actual
Verus/Z3 compatibility. A solver version error or a proxy-only network is a
compatibility failure to report, not a reason to bypass the download policy or
trust a successful `--version` alone.

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
- [x] (2026-11-01) Refresh the tooling pin against upstream `main` and revise
  installation, Kani execution, multi-file proofs, solver setup, and evidence.
- [x] (2026-11-01) The user explicitly approved implementation of this plan.
- [x] (2026-11-01 04:05Z) M1: Establish pinned compatibility and provenance.
      Actual installation and positive/negative proof results are recorded below.
- [ ] M2: Add tested local command boundaries.
- [ ] M3: Demonstrate both tools and reject deliberate faults.
- [ ] M4: Integrate CI, documentation, and final validation.

## Surprises & discoveries

The local Netsuke default ref was older than the live donor revision. The older
snapshot lacked Verus pins and proofs; the live revision includes them and
updates the shared tooling pin. The source inventory above uses the live
revision, not the stale snapshot.

The delegated Verus runner retains a legacy default proof path and fallback
binary selection. Explicit Axinite entry points and observable version output
remain necessary. Netsuke's version-only aggregate must not be described as
proof execution when adapted.

The 2026-11-01 upstream refresh adds a qualified Kani execution boundary and
prebuilt installers that the original donor pin did not expose. Multi-file
Verus invocation and closed-pipe exit handling also changed. The newer package,
rather than old donor recipes, now owns these behaviours; all four milestones
and V1, V2, and V5 below account for that change.

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

- (2026-11-01) Supersede the historical tooling pin with upstream commit
  `592558f7183b554fc749696246984c5d7300d029`. Delegate installation and Kani
  execution to the package; qualify the pinned Z3/Verus pair, repeated proof
  arguments, and failure propagation at Axinite's boundary. R1–R4 retain their
  scope; no production proof or new runtime dependency is authorized.

- (2026-11-01) Accept the user’s explicit approval and begin the planned
  implementation within the recorded constraints and tolerances.

- (2026-11-01) The M1 probe demonstrated Kani 0.67.0 and the pinned Verus/Z3
  pair under managed compiler admission. Keep the local Cargo configuration
  warning documented; no root toolchain or wrapper change is needed.

## Plan of work

### M1 — Pin and demonstrate compatibility

R1 and R2 apply. Re-read the exact donor files and package CLI documentation.
Record provenance, licence compatibility, versions, digests, toolchain
requirements, and supported architecture in this plan. Use Python 3.14 and
confirm that the installed CLI exposes `kani exec`, `z3 install`, and repeated
`--proof-file`. Do not infer an installable 0.2.0 release from the guide title.

Use the prebuilt Kani path and preserve the shared Cargo/rustup homes. Record
both the frontend location and the selected release root after setup; inspect
its `toolchain/` link and matching runtime without rewriting either. Verify
that a non-selecting verifier argument for `kani-full` (for example,
`--output-format=regular`) runs every harness, since `kani exec` rejects an
empty argument list. Unsupported Kani versions need separate qualification;
`--compile` does not expand `kani exec`'s supported release set.

Install Verus using the retained donor pin/checksum, install package-pinned Z3,
and pass the absolute solver path explicitly. Run `z3 --version` and a real
Verus proof to establish solver compatibility. If the Verus release requires a
different Z3, record the failure and propose an upstream-supported pin change
for approval instead of substituting an unpinned system solver.

In disposable source scratch below the worktree's ignored `target/`, run a
minimal Kani assertion and a minimal Verus lemma through the pinned package. Use
`kani exec --installation <release-root> --cwd <proof-package>` with literal
repeated `--extra-arg` values. Use the existing Cargo home and managed compiler
admission. Record the exact commands, selected compilers, wrapper behaviour,
and exit codes. Probe both a true assertion and an intentionally false
assertion; a tool that only prints its version does not satisfy the experiment.

The plateau is a documented compatibility decision with no application change.
Acceptance is real successful verification plus rejected false assertions for
both tools. Missing tools or a refused compiler invocation are inconclusive,
not proof failures. If compatibility fails, preserve logs, mark the plan
BLOCKED, and propose a narrower supported boundary. Remove only owned scratch
sources on recovery; never delete shared caches. M2–M4 remain outstanding.

#### M1 execution evidence

On 2026-11-01 the clean-environment probe used the pinned package commit
`592558f7183b554fc749696246984c5d7300d029` with Python 3.14. The host is Linux
x86-64 with glibc 2.39. The Kani installer downloaded and verified the
package-pinned frontend and bundle, installed frontend binaries under
`target/prover-m1/frontend`, and set up the release under
`target/prover-m1/kani-home/kani-0.67.0`. Its `toolchain` symlink resolves to
the shared `nightly-2025-11-21-x86_64-unknown-linux-gnu` installation. The
Cargo home and managed `RUSTC_WRAPPER` remained unchanged. The package's ISC
licence permits use with its notice; Axinite declares `MIT OR Apache-2.0`. The
plan imports package behaviour through the CLI, not copied source.

The Verus installer verified donor version `0.2026.10.04.426d8b0` against
SHA-256 `624018662868563a7bec0dd07be5e4dd0c3d61124b4b55105765884bf2d4d1a0`. Z3
4.16.0 installed from the pinned package and passed `z3 --version`. Verus
selected toolchain `1.98.1-x86_64-unknown-linux-gnu` and verified a valid
integer lemma with `2 verified, 0 errors`; an impossible postcondition returned
exit 1 with `1 errors`. This establishes compatibility for the observed host
and lemmas, not for arbitrary future proofs.

Kani's `kani exec` used the selected release root, scratch proof working
directory, `--extra-arg=--output-format=regular`, and the real admission
wrapper. The named `intersection_is_subset` harness returned exit 0 and one
successful verification. `false_union_is_subset` returned exit 1 with its
assertion marked FAILURE. With no `--harness`, the same output-format argument
ran both: one success, one failure, two total, exit 1. The inherited root
`.cargo/config.toml` produced an `unused config key unstable.embed-metadata`
warning from Kani's older Cargo; neither proof result was suppressed nor
misclassified. The first Kani build spent 9 minutes 46 seconds in Cargo under a
healthy, busy shared admission pool.

The following reproduces the exact prover arguments. The original invocation
used a clean environment without Git credential overrides, retained managed
compiler wrappers, and used `uv tool run --python 3.14 --from` the pinned
source. Install inputs were `tools/kani/VERSION`, `tools/verus/VERSION`, and
`tools/verus/SHA256SUMS` under the scratch root, copied from the versions and
checksum above. The Kani package was an edition-2021 standalone Cargo package
with `[workspace]`. Its two `#[kani::proof]` harnesses generated arbitrary
`requested: u8` and `permitted: u8`, then asserted respectively that
`(requested & permitted) & !permitted == 0` and
`(requested | permitted) & !permitted == 0`. The Verus files each imported
`vstd::prelude::*` and supplied `fn main() {}` inside `verus!`; their proof
functions ensured respectively `x + 0 == x` and `x + 1 == x` for `x: int`.

```bash
probe="$PWD/target/prover-m1"
source='git+https://github.com/leynos/rust-prover-tools@592558f7183b554fc749696246984c5d7300d029'
KANI_HOME="$probe/kani-home" uv tool run --python 3.14 --from "$source" prover-tools kani install \
  --repo-root "$probe" --install-dir "$probe/frontend" --cache-dir "$probe/kani-cache" --no-compile
uv tool run --offline --python 3.14 --from "$source" prover-tools verus install \
  --repo-root "$probe" --install-dir "$probe/verus-install"
uv tool run --offline --python 3.14 --from "$source" prover-tools z3 install --repo-root "$probe"
export VERUS_Z3_PATH="$probe/.cache/z3/4.16.0/z3"
uv tool run --offline --python 3.14 --from "$source" prover-tools verus run --repo-root "$probe" \
  --install-dir "$probe/verus-install" --proof-file "$probe/verus/valid.rs"
uv tool run --offline --python 3.14 --from "$source" prover-tools verus run --repo-root "$probe" \
  --install-dir "$probe/verus-install" --proof-file "$probe/verus/invalid.rs"
uv tool run --offline --python 3.14 --from "$source" prover-tools kani exec \
  --installation "$probe/kani-home/kani-0.67.0" --cwd "$probe/kani" \
  --extra-arg=--harness=intersection_is_subset --extra-arg=--output-format=regular
uv tool run --offline --python 3.14 --from "$source" prover-tools kani exec \
  --installation "$probe/kani-home/kani-0.67.0" --cwd "$probe/kani" \
  --extra-arg=--harness=false_union_is_subset --extra-arg=--output-format=regular
uv tool run --offline --python 3.14 --from "$source" prover-tools kani exec \
  --installation "$probe/kani-home/kani-0.67.0" --cwd "$probe/kani" --extra-arg=--output-format=regular
```

The initial Kani install and CLI discovery used network-enabled `uv`;
subsequent runs used `--offline`. Expected exit statuses are 0 for installation
and valid proofs, and 1 for the invalid proofs and all-harness Kani command.
Full local transcripts remain in `/tmp/axinite-prover-m1-*.out`; the inputs
under ignored `target/prover-m1/` are scratch evidence, not the checked-in M3
fixtures. Conformance: R1 and R2 are advanced, the pinned CLI and trust
boundaries are unchanged, and no runtime API, dependency, persistent format, or
architecture deviation was introduced. M2–M4 remain open.

### M2 — Add local installation and execution contracts

R1 and R2 apply. Add the three pin/checksum files, adapt the shared package
variables and local targets in `Makefile`, and ignore `.verus/`,
`.cache/kani/`, and `.cache/z3/` installation/archive output. These archive
caches are not separate Cargo homes. Explicitly pass version-file overrides to
the CLI; merely echoing a Make variable does not make the installer honour it.

Add `tests/workflow_contracts/prover_tools_test.py` first. Use temporary fake
executables to record arguments, environment, selected proof files, and exit
codes. Exercise missing/malformed pins, version mismatch, quoted command
arguments, custom pin paths, non-zero verifier exits, and sensitive flag
redaction. Run `make test-workflow-contracts` and capture the expected red
failure before editing recipes. Implement the smallest recipes that pass.

Provide `install-kani`, `kani-check`, `kani`, `kani-full`, `install-verus`,
`install-z3`, `verus`, and `formal-pr`. Use `kani install --no-compile` and
`z3 install` through the pinned package. Both Kani proof targets must invoke
`kani exec` with the selected release root and the fixture's absolute working
directory; forward verifier arguments as repeated literal `--extra-arg` options
rather than shell-evaluated text. `kani` selects the smoke harness explicitly;
`kani-full` runs every harness in the checked-in verification fixture;
`formal-pr` runs `kani` followed by `verus` sequentially. Both Kani run targets
must check the pin first. No Netsuke-specific `kani-ir` alias is needed.

Set an explicit default Verus entry point and retain the single-file override
`VERUS_PROOF_FILE=verus/monty_capabilities.rs`. Provide an explicit repeated
`--proof-file` route for multiple entry points through `VERUS_FLAGS`; do not
split paths out of an environment string or glob the directory. Define
precedence in one place: an explicit repeated CLI list replaces the default or
single-file selection, rather than appending a hidden default proof. Test paths
containing spaces and a second file that fails. If Make's quoting needs a
helper, keep it a small argument adapter, not another proof runner.

Add `Z3_INSTALL_DIR` and pass an absolute `VERUS_Z3_PATH` consistently for
local and CI runs, including negative controls. Make installation explicit; a
missing solver is a setup error, not a successful proof rejection. Test
install/cache path overrides and installer failure propagation using fake
commands. Do not reimplement or exhaustively retest the upstream downloader;
real installs in M1 and M4 exercise its published interface.

Test Kani routing with the real pinned CLI against a faithful local boundary
and in M1's real proof run. Keep wrappers unchanged, exercise encoded flags
(including an empty encoded value), and check that supported development flags
are removed only in the child while compatible flags remain. An unsupported
routing argument must fail before proof execution. Retain useful source,
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
Include a Verus sequence with a passing file, a failing file, and a later file:
the failure must name the second file, report the first as proved, and never
run the third. A missing later path must prevent all proof execution. Test a
closed output reader with passing and failing subprocesses and capture the
prover process's status, not the final pipeline stage. These tests exercise
Axinite's command/argument plumbing and failure classification against the
pinned CLI; upstream owns the streaming implementation.

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

Install prebuilt Kani in `kani-smoke` using the shared package, without copying
Netsuke's download script. In `verus-proofs`, install both Verus and Z3 and set
the same absolute solver path used locally. Run each tool's positive and
corresponding negative controls, including the multi-file Verus case, and
retain concise tool/solver version and results logs on failure. Use 30-minute
job bounds initially. Add workflow-contract tests before workflow
implementation, covering events, permissions, pins, timeouts, and commands. Run
those tests red, implement, then run green. Do not copy Netsuke's host-specific
runner selection or change repository branch protection here.

Update `docs/developers-guide.md`,
`docs/formal-verification-methods-in-axinite.md`, and `docs/contents.md` with
installation, exact target semantics, compiler boundaries, expected failures,
and instructions for adding a harness or several Verus entry points. Document
Python 3.14, the distinct Kani frontend and release paths, explicit compile
fallback limits, Z3 configuration, and direct-network requirements. Review
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
matching against an echoed pin. Exercise prebuilt-route selection, solver-path
forwarding, and repeated proof argument precedence. Actual Kani and Z3 archive
digests are supplied by the pinned package; Verus's digest remains in Axinite.
A real Verus proof with the installed solver discharges the compatibility
assumption, while a missing solver must remain a setup failure.

V2, for R2/M2–M4: ordinary builds retain their compiler flags and provers do
not enter runtime dependencies. Axinite's contract tests establish that Make
calls the pinned `kani exec` with the correct installation, working directory,
and literal arguments. M1's real CLI experiment establishes child-only flag
normalization with admission wrappers intact. Include plain flags, encoded
flags, an explicitly empty encoded value, and unsupported routing rejection. A
mutation that bypasses `kani exec` must fail the recipe test. `make all` and
manifest inspection provide integration evidence. The package's normalization
algorithm, compilers, and package manager remain trusted external interfaces;
do not copy their implementation or claim nested coverage-wrapper support.

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
installation failure is never classified as a successful negative control. For
multi-file runs, all requested files must pass before aggregate success; assert
preflight failure, ordered execution, first-failure reporting, and no later
invocation. For closed-pipe cases, assert the prover subprocess status for both
a valid and an invalid proof, with setup/syntax failures excluded. The
pipeline's status alone is not the oracle. The real pinned CLI and solver are
external assumptions exercised by these repository integration tests.

## Concrete validation and recovery

Run commands from the repository root, one gate at a time, with `pipefail` and
logs under `/tmp`. During implementation the relevant sequence is:

```bash
set -o pipefail
make test-workflow-contracts 2>&1 | tee /tmp/prover-contracts-axinite.out
make install-kani 2>&1 | tee /tmp/prover-install-kani-axinite.out
make install-verus 2>&1 | tee /tmp/prover-install-verus-axinite.out
make install-z3 2>&1 | tee /tmp/prover-install-z3-axinite.out
make formal-pr 2>&1 | tee /tmp/prover-formal-pr-axinite.out
make kani-full 2>&1 | tee /tmp/prover-kani-full-axinite.out
make test-prover-tools 2>&1 | tee /tmp/prover-negative-controls-axinite.out
make fmt 2>&1 | tee /tmp/prover-fmt-axinite.out
make all 2>&1 | tee /tmp/prover-all-axinite.out
make markdownlint 2>&1 | tee /tmp/prover-markdownlint-axinite.out
make nixie 2>&1 | tee /tmp/prover-nixie-axinite.out
git diff --check
```

Do not truncate these gate logs with `head`. When a test intentionally closes a
reader, capture the prover subprocess return code directly, or read Bash
`PIPESTATUS[0]` immediately in both branches of an `if` pipeline. A Make target
may map a recipe failure to its own non-zero status; exact prover-status tests
must observe the CLI subprocess, not assume Make preserves that number.

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
pinned revisions. The 2026-11-01 revision updates the planned integration to
the newer upstream package; compatibility and proof execution remain M1–M4
work, and no installer or runtime implementation has started.

## Revision notes

- (2026-10-28) Initial draft based on the live Netsuke source, the delegated
  package contract, Axinite's build standard, and the Monty proof obligations.
- (2026-11-01) Update the tooling pin to upstream `main` commit `592558f7`.
  Replace prospective local flag handling and copied installer logic with the
  upstream Kani execution and bounded installer interfaces. Add Z3 setup,
  multi-file Verus checks, and closed-pipe status evidence throughout M1–M4 and
  V1/V2/V5. Preserve the Netsuke donor snapshot and Monty proof scope;
  implementation was subsequently approved on 2026-11-01.
- (2026-11-01) Record explicit user approval before implementation begins.
- (2026-11-01) Record the successful M1 compatibility plateau and its limits.
- (2026-11-01) Add reproducible M1 commands and correct review wording.
