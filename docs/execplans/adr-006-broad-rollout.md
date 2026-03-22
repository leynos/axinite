# Roll out ADR 006 across the remaining dyn-backed async trait families

This ExecPlan (execution plan) is a living document. The sections
`Constraints`, `Tolerances`, `Risks`, `Progress`, `Surprises & Discoveries`,
`Decision Log`, and `Outcomes & Retrospective` must be kept up to date as work
proceeds.

Status: DRAFT

## Purpose / big picture

The earlier async-trait migration work proved two things. First, the easy wins
were small: only `WasmChannelStore` and `SuccessEvaluator` could move directly
to native async traits. Second, Architectural Decision Record (ADR) 006's
dual-trait pattern works for dyn-backed interfaces: `McpTransport`,
`SettingsStore`, and `SoftwareBuilder` now preserve existing `Arc<dyn Trait>`
consumers while letting implementations move off `#[async_trait]`.

This follow-up plan turns that pilot into a broader refactor. After this work,
the repository should use one repeatable migration pattern for the remaining
dyn-backed async interfaces, reduce the direct `#[async_trait]` footprint across
the highest-value trait families, and preserve the current object-safe API
surfaces for callers. Success is observable in three ways:

1. The targeted trait families compile and test with their dyn-facing trait
   names unchanged, while their concrete implementations switch to native async
   sibling traits.
2. `rg -n "async-trait|async_trait" src` reports materially fewer production
   uses than the current branch baseline.
3. The normal repository gates and at least one timing sample per rollout wave
   complete successfully, proving the refactor did not trade compile-time gains
   for behavioural regressions.

## Repository orientation

The work already done on this branch is documented in
`docs/execplans/migrate-async-trait.md` and
`docs/adr-006-dual-trait-pattern-for-dyn-backed-async-interfaces.md`. The
first file captures the audit history, the pilot families, and the remaining
blocked traits. The ADR records the accepted migration shape: keep the existing
dyn-facing trait name on the object-safe boundary, add a `Native*` sibling for
implementation ergonomics, and bridge the two with a blanket adapter.

Most of the remaining migration surface sits in four clusters:

- Narrow internal traits with limited consumers, such as
  `LoopDelegate` in `src/agent/agentic_loop.rs`,
  `SelfRepair` in `src/agent/self_repair.rs`,
  `TaskHandler` in `src/agent/task.rs`,
  `ChannelSecretUpdater` in `src/channels/channel.rs`,
  `HttpInterceptor` in `src/llm/recording.rs`, and
  `CredentialResolver` in `src/sandbox/proxy/http.rs`.
- Infrastructure-facing extension seams, such as
  `EmbeddingProvider` in `src/workspace/embeddings.rs`,
  `NetworkPolicyDecider` in `src/sandbox/proxy/policy.rs`,
  `Hook` in `src/hooks/hook.rs`,
  `Observer` in `src/observability/traits.rs`,
  `Tunnel` in `src/tunnel/mod.rs`,
  `SecretsStore` in `src/secrets/store.rs`, and
  `TranscriptionProvider` in `src/transcription/mod.rs`.
- High-fanout core extensibility traits, notably
  `Channel`, `Tool`, `LlmProvider`, and `Database`.
- The supporting documentation set in
  `docs/execplans/migrate-async-trait.md`,
  `docs/adr-006-dual-trait-pattern-for-dyn-backed-async-interfaces.md`, and
  `docs/contents.md`.

The repository rules for this branch remain strict: keep the dyn-facing public
surfaces stable where possible, update the docs in the same branch as the code
changes, and gate every commit.

## Constraints

- Preserve the existing dyn-facing trait names for migrated families unless a
  deeper architectural change is explicitly approved. The plan assumes callers
  such as `Arc<dyn Tool>` and `Arc<dyn Database>` continue to compile without
  broad call-site rewrites.
- Do not change the repository minimum toolchain or Rust edition. The plan is
  constrained to Rust 1.92 and Edition 2024.
- Avoid new dependencies. The accepted path is the local ADR 006 dual-trait
  pattern, not `trait_variant` or a new helper crate.
- Keep each migration wave small enough to validate independently. No single
  commit should span unrelated trait families.
- Update the governing docs whenever a wave changes the migration guidance,
  family ordering, or the remaining dependency rationale for `async-trait`.
- Preserve behaviour across PostgreSQL and libSQL backends. Any database-family
  migration must continue to satisfy both backends and their shared `Database`
  abstraction.
- Treat compile-time evidence as part of the deliverable. A broader refactor
  without before-and-after evidence is incomplete.

## Tolerances (exception triggers)

- Scope: if any single migration wave needs more than 18 files or roughly 900
  net lines of change, stop and split the wave before proceeding.
- Interface: if a migration requires renaming a public dyn-facing trait,
  changing a stable trait method signature at the call boundary, or changing
  user-visible configuration contracts, stop and escalate.
- Dependencies: if the work appears to require a new crate or a shared helper
  module outside the existing subsystem ownership boundaries, stop and justify
  the trade-off in `Decision Log` before proceeding.
- Iterations: if one wave fails the same gate three times without a clear
  defect fix between attempts, stop and reassess the wave boundary.
- Evidence: if `cargo check --timings` or an equivalent observable compile-time
  sample cannot be captured for a wave, do not mark that wave complete.
- Ambiguity: if a trait family sits on a boundary where preserving the current
  dyn-facing API would cause unreasonable adapter complexity, stop and present
  the alternatives before changing course.

## Risks

- Risk: the `Database` family combines dyn-backed supertraits with two backends
  and a large call graph.
  Severity: high
  Likelihood: high
  Mitigation: treat `Database` as the final migration wave, after the narrower
  infrastructure seams establish a stable pattern for shared boxed-future
  aliases and blanket adapters.

- Risk: `Tool` and `LlmProvider` have enough downstream implementations and test
  doubles that a naïve mechanical rewrite could create widespread noise.
  Severity: high
  Likelihood: medium
  Mitigation: migrate these families only after extracting a precise inventory
  of implementations and test scaffolding, then split production adapters from
  test-only conversions where needed.

- Risk: native async sibling traits can silently lose the implicit `Send`
  guarantee that `#[async_trait]` provided.
  Severity: high
  Likelihood: medium
  Mitigation: require every migrated native sibling method to return
  `impl Future<...> + Send`, and add compile-time checks or tests in each wave
  where spawned or cross-thread use exists.

- Risk: compile-time gains may be smaller than expected if the refactor lands
  mostly in low-volume trait families first.
  Severity: medium
  Likelihood: medium
  Mitigation: capture timing evidence per wave and reorder later waves if the
  early data shows a different high-value target.

- Risk: scope drift will tempt the implementer to "clean up" unrelated async
  code while touching the same files.
  Severity: medium
  Likelihood: high
  Mitigation: keep each commit family-scoped, update `Progress` before each new
  wave, and defer unrelated cleanup unless it directly unblocks a gate.

## Implementation plan

### Milestone 1: Normalize the migration playbook and baseline evidence

Start by turning the pilot knowledge into a repeatable playbook. Re-read
`docs/execplans/migrate-async-trait.md` and the ADR, then capture a fresh
baseline for:

- the current `async-trait` usage count in production files;
- the exact remaining dyn-backed trait families; and
- one compile-time sample that can later be compared against post-wave data.

Update the existing migration execplan with the baseline snapshot if it has
drifted. If the team now prefers one shared boxed-future alias helper rather
than subsystem-local aliases, this is the point to decide it explicitly and
record the choice before more families are migrated.

Observable result: the docs state the current starting line, and there is a
baseline log showing both the current async-trait footprint and one timing
sample.

### Milestone 2: Convert the narrow internal dyn-backed traits

Use the pilot pattern on the small internal families first:
`LoopDelegate`, `SelfRepair`, `TaskHandler`, `ChannelSecretUpdater`,
`HttpInterceptor`, and `CredentialResolver`.

For each family:

1. Keep the existing dyn-facing trait name on the object-safe boundary.
2. Add a `Native*` sibling trait in the owning module.
3. Bridge the sibling trait back into the dyn-facing trait with a blanket
   implementation.
4. Move concrete implementations and test doubles to the sibling trait unless a
   direct dyn-facing implementation is genuinely simpler.
5. Verify the existing dyn-backed call sites remain unchanged or nearly
   unchanged.

These families are small enough to prove whether the pilot pattern scales
across several subsystems without the blast radius of `Tool` or `Database`.

Observable result: those trait families no longer use `#[async_trait]` in their
production definitions and concrete implementations, and the affected module
tests still pass.

### Milestone 3: Convert the infrastructure-facing extension seams

Migrate the next tier of traits:
`EmbeddingProvider`, `NetworkPolicyDecider`, `Hook`, `Observer`, `Tunnel`,
`SecretsStore`, and `TranscriptionProvider`.

This wave is larger because these interfaces cross more subsystem boundaries and
have more implementations, but they are still more tractable than the core
extensibility surfaces. Treat each family as its own commit unless two families
share the same module and the same gate impact. Reuse the same sibling naming
and boxing pattern from the pilot and Milestone 2.

Special handling:

- `TranscriptionProvider` and `Tunnel` both have multiple concrete backends, so
  capture the implementation inventory before editing.
- `Hook` and `Observer` are extension surfaces that are likely to have test
  doubles in multiple modules. Expect to update test fixtures and registry
  wiring in the same commit.
- `EmbeddingProvider` may sit close to background or multi-threaded work, so
  verify the `Send` contract explicitly.

Observable result: these extension seams retain their dyn-facing usage model but
move the majority of implementation code off `#[async_trait]`.

### Milestone 4: Convert the high-fanout core traits

With the pattern stabilized, tackle the biggest remaining wins:
`Channel`, `Tool`, `LlmProvider`, and the `Database` family.

This milestone is intentionally split into four separate sub-waves. Do not
attempt them all at once.

- `Channel`: convert `src/channels/channel.rs` and the concrete channel modules
  that implement it, while preserving the current registration path.
- `Tool`: convert the core tool trait and the built-in tool implementations,
  along with the tool registry and test fixtures.
- `LlmProvider`: convert the provider trait and the concrete providers, then
  validate smart routing, retries, failover, and caching layers.
- `Database`: convert `src/db/mod.rs` and the supertrait family only after the
  smaller waves prove the pattern at scale, then migrate the PostgreSQL and
  libSQL backends in lockstep.

The `Database` sub-wave is the highest-risk portion of the plan. Expect to stop
and reassess if the adapter surface becomes too noisy or if backend parity
starts to drift.

Observable result: the largest remaining production users of `#[async_trait]`
are converted without changing the dominant dyn-facing architecture.

### Milestone 5: Clean up dependency and documentation state

After each conversion wave, update the docs. At the end of the series:

- refresh `docs/execplans/migrate-async-trait.md` with the new completed waves,
  remaining blocked families, and current verified usage count;
- update
  `docs/adr-006-dual-trait-pattern-for-dyn-backed-async-interfaces.md` if the
  broad rollout reveals refinements to the accepted pattern;
- keep `docs/contents.md` synchronized with any new execplans or related docs;
  and
- re-check whether `async-trait` is still needed as a direct dependency.

Only remove `async-trait` from `Cargo.toml` when a fresh tree audit shows that
no production trait family still requires it.

Observable result: the docs accurately describe the new migration frontier, and
the dependency state matches the code.

## Validation and evidence

Every wave should capture the same evidence pattern so a novice can tell
whether the migration really succeeded:

1. Before editing, record the relevant `async-trait` usage inventory for that
   family and save the output to a `/tmp` log.
2. After editing, run the smallest proving gate first for the touched area
   where available.
3. Run the repository gates through `tee`, keeping logs for
   `make check-fmt`, `make typecheck`, `make lint`, and `make test`.
4. Capture at least one compile-time sample for the wave, such as
   `cargo check --timings`, and save it to a log path or generated report.
5. Re-run `rg -n "async-trait|async_trait" src` and include the delta in the
   progress notes.

Expected command pattern:

```plaintext
set -o pipefail; make check-fmt | tee /tmp/check-fmt-axinite-<wave>.out
set -o pipefail; make typecheck | tee /tmp/typecheck-axinite-<wave>.out
set -o pipefail; make lint | tee /tmp/lint-axinite-<wave>.out
set -o pipefail; make test | tee /tmp/test-axinite-<wave>.out
set -o pipefail; cargo check --timings | tee /tmp/timings-axinite-<wave>.out
```

For documentation-only updates within the series, also run:

```plaintext
set -o pipefail; bunx markdownlint-cli2 <changed-docs> | tee /tmp/markdownlint-axinite-<wave>.out
set -o pipefail; git diff --check | tee /tmp/diff-check-axinite-<wave>.out
```

## Progress

- [ ] Milestone 1: Normalize the migration playbook and baseline evidence
- [ ] Milestone 2: Convert the narrow internal dyn-backed traits
- [ ] Milestone 3: Convert the infrastructure-facing extension seams
- [ ] Milestone 4: Convert the high-fanout core traits
- [ ] Milestone 5: Clean up dependency and documentation state

## Surprises & Discoveries

- 2026-03-22: The earlier pilot work showed that the original migration scope
  estimate was too optimistic. The broad rollout must assume that most of the
  remaining value sits behind dyn-backed interfaces, not concrete-only traits.
- 2026-03-22: `McpTransport`, `SettingsStore`, and `SoftwareBuilder` provide a
  credible reference implementation for the sibling-trait pattern, so the next
  plan can focus on family ordering and validation discipline rather than
  debating the migration mechanism again.

## Decision Log

- 2026-03-22: Chose a wave-based refactor rather than a single large rewrite.
  Rationale: the pilot succeeded, but the remaining families have very
  different blast radii. Separate waves keep failures local and make compile-
  time evidence easier to interpret.
- 2026-03-22: Deferred the `Database` family to the end of the plan. Rationale:
  it combines the highest fan-out with backend parity risk and supertrait
  complexity, so earlier waves should absorb the pattern refinements first.
- 2026-03-22: Treated documentation and timing evidence as part of the feature,
  not post-hoc cleanup. Rationale: this refactor exists to improve build
  behaviour while preserving architecture, so the evidence must stay alongside
  the code changes.

## Outcomes & Retrospective

This section is intentionally blank until execution begins. When work lands,
record which families were migrated, which ones remained blocked, what evidence
was captured, and whether the broad rollout changed the recommendation for
keeping or removing `async-trait`.
