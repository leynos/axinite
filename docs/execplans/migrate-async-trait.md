# Migrate from async-trait to native async traits

**Branch:** `migrate-async-trait`
**Date:** 2026-03-15
**Status:** In progress
**Estimated impact:** Reduced proc-macro expansion overhead for the small
subset of async traits that are not used as trait objects

## Big picture

The `async-trait` proc macro is used 158 times across 74 source files.
Each use generates boxing and dynamic dispatch boilerplate at compile time.
Since the project targets Rust 1.92 (which supports native `async fn` in
traits, stabilized in Rust 1.75), the currently verified migration scope
is **5 of 158 uses**. More may become migratable later, but only after
removing trait-object usage or piloting ADR 006's dual-trait pattern.

However, native async traits are **not object-safe** — it is not possible
to write `dyn MyAsyncTrait` without boxing the future. This project uses
`dyn Trait` extensively (246 occurrences across 65 files for the core
extensibility traits). This means the migration is **partial**: traits used
as trait objects must either remain with `async-trait` or adopt a manual
boxing pattern.

## Approval gates

- Plan approved
  Acceptance criteria: the migration scope, blocked surfaces, and roadmap
  boundaries are explicit enough that later batches can proceed without
  guessing which `async-trait` uses are in scope.
  Sign-off: human reviewer approves the ExecPlan before implementation starts
  or before the next migration batch proceeds.
- Implementation complete
  Acceptance criteria: the planned migration batch lands with the intended
  trait classifications, blocked dyn-backed surfaces remain outside scope, and
  any required follow-on docs are updated in the same pass.
  Sign-off: implementer marks the batch complete before final validation.
- Validation passed
  Acceptance criteria: the required repository gates for the migration batch
  pass, and the final plan notes record the command evidence and any residual
  blocked work.
  Sign-off: implementer records the gate results immediately before commit and
  push.
- Docs synced
  Acceptance criteria: the execplan, linked ADRs, roadmap references, and
  index entries reflect the delivered migration scope before the plan is
  marked complete.
  Sign-off: implementer completes the documentation pass as the final
  pre-commit step.

## Constraints

- Rust edition 2024, minimum version 1.92.
- Traits used as `dyn Trait` (boxed trait objects) cannot use native async
  methods directly. Architectural decision record (ADR) 006 adopts a
  dual-trait pattern for those surfaces.
- The migration must be incremental — it is not feasible or desirable to
  change all 158 uses in a single commit.
- Each batch of changes must pass `make all`.

## Trait classification

### Core extensibility traits (used as `dyn Trait` — blocked pending ADR 006)

These traits are used as trait objects throughout the codebase and cannot
trivially migrate to native async traits. Until the ADR 006 pilot lands,
they remain on `async-trait`:

- `Database` (`src/db/mod.rs`) — 10 `dyn` references
- `Channel` (`src/channels/channel.rs`) — used via `dyn Channel`
- `Tool` (`src/tools/tool.rs`) — used via `dyn Tool`
- `LlmProvider` (`src/llm/provider.rs`) — 19 `dyn` references in
  `src/llm/mod.rs` alone
- `EmbeddingProvider` (`src/workspace/embeddings.rs`)
- `NetworkPolicyDecider` (`src/sandbox/proxy/policy.rs`)
- `Hook` (`src/hooks/hook.rs`)
- `Observer` (`src/observability/traits.rs`)
- `Tunnel` (`src/tunnel/mod.rs`)
- `SecretsStore` (`src/secrets/store.rs`)
- `TranscriptionProvider` (`src/transcription/mod.rs`)

### Concrete-only traits (safe to migrate)

The initial plan overestimated this bucket. A direct audit on
2026-03-20 found that many "internal" traits are still used as trait
objects and therefore cannot yet migrate. Examples:

- `LoopDelegate` is passed as `&dyn LoopDelegate`
- `SelfRepair` is stored as `Arc<dyn SelfRepair>`
- `TaskHandler` is stored as `Arc<dyn TaskHandler>`
- `HttpInterceptor` is stored as `Arc<dyn HttpInterceptor>`
- `CredentialResolver` is stored as `Arc<dyn CredentialResolver>`
- `SoftwareBuilder` is stored as `Arc<dyn SoftwareBuilder>`
- `TranscriptionProvider` is stored as `Box<dyn TranscriptionProvider>`
- `McpTransport` is stored as `Arc<dyn McpTransport>`
- `WasmToolStore` is passed as `&dyn WasmToolStore`

The traits currently confirmed safe to migrate are:

- `WasmChannelStore` (`src/channels/wasm/storage.rs`)
- `SuccessEvaluator` (`src/evaluation/success.rs`)

### `impl` blocks of core traits

Even though the trait definition must keep `#[async_trait]`, the `impl`
blocks on concrete types **also** need `#[async_trait]` because the trait
signature expects `Pin<Box<dyn Future>>` return types. These cannot be
migrated independently of the trait definition.

## Migration strategy

### Approach: Bottom-up, concrete-only first

1. Identify traits that are **never** used as `dyn Trait`.
2. Remove `#[async_trait]` from those trait definitions and all their
   `impl` blocks.
3. Leave core extensibility traits unchanged (they require `async-trait`
   for object safety).
4. For dyn-backed traits, follow ADR 006's dual-trait pattern rather than
   evaluating `trait_variant` as an active migration path.

### Phase 1: Audit and classify every `#[async_trait]` use

- [x] For each trait with `#[async_trait]`, determine whether it is ever
  used as `dyn Trait`
- [x] Produce a table of trait name, file, dyn-used (yes/no),
  migratable (yes/no)
- [x] Identify the subset that can be migrated

Audit snapshot as of 2026-03-20:

- `WasmChannelStore` in `src/channels/wasm/storage.rs`:
  no trait-object usage found, migratable now.
- `SuccessEvaluator` in `src/evaluation/success.rs`:
  no trait-object usage found, migratable now.
- `ConversationStore`, `JobStore`, `SandboxStore`, `RoutineStore`,
  `ToolFailureStore`, and `WorkspaceStore` in `src/db/mod.rs`:
  blocked because `Database` depends on them as supertraits.
- `LoopDelegate` in `src/agent/agentic_loop.rs`:
  blocked by `&dyn LoopDelegate`.
- `SelfRepair` in `src/agent/self_repair.rs`:
  blocked by `Arc<dyn SelfRepair>`.
- `TaskHandler` in `src/agent/task.rs`:
  blocked by `Arc<dyn TaskHandler>`.
- `ChannelSecretUpdater` in `src/channels/channel.rs`:
  blocked by `Arc<dyn ChannelSecretUpdater>`.
- `HttpInterceptor` in `src/llm/recording.rs`:
  blocked by `Arc<dyn HttpInterceptor>`.
- `CredentialResolver` in `src/sandbox/proxy/http.rs`:
  blocked by `Arc<dyn CredentialResolver>`.
- `SoftwareBuilder` in `src/tools/builder/core.rs`:
  blocked by `Arc<dyn SoftwareBuilder>`.
- `McpTransport` in `src/tools/mcp/transport.rs`:
  blocked by `Arc<dyn McpTransport>`.

Re-audit snapshot as of 2026-03-21:

- No new safe native-async candidates were found after the first batch.
- `ConversationStore`, `JobStore`, `SandboxStore`, `RoutineStore`,
  `ToolFailureStore`, and `WorkspaceStore` still have no direct `dyn`
  call sites, but they remain blocked because `Database` inherits them as
  supertraits and `Database` is used as `Arc<dyn Database>`.
- `SettingsStore` remains blocked by direct `Arc<dyn SettingsStore>` usage.
- The remaining async traits still fall into one of two categories:
  directly used as trait objects, or inherited by a trait object that
  preserves the object-safety requirement.

Re-audit snapshot as of 2026-03-21 after Phase 3 expansion:

- `SettingsStore` in `src/db/mod.rs` is now piloted with ADR 006's
  dual-trait pattern while preserving both direct `Arc<dyn
  SettingsStore>` consumers and the `Database` supertrait boundary.
- `SoftwareBuilder` in `src/tools/builder/core/domain.rs` is now piloted
  with ADR 006's dual-trait pattern while preserving
  `Arc<dyn SoftwareBuilder>` consumers in self-repair and the build tool.

Re-audit snapshot as of 2026-03-22 during ADR 006 broad rollout execution:

- `LoopDelegate` in `src/agent/agentic_loop.rs` now uses ADR 006's dual-trait
  pattern while preserving `&dyn LoopDelegate` call sites in the shared
  agentic loop and the chat, job, and container delegates.
- `SelfRepair` in `src/agent/self_repair.rs` now uses ADR 006's dual-trait
  pattern while preserving `Arc<dyn SelfRepair>` consumers in the repair task.
- `TaskHandler` in `src/agent/task.rs` now uses ADR 006's dual-trait pattern
  while preserving `Arc<dyn TaskHandler>` consumers in scheduler background
  tasks.

Re-audit snapshot as of 2026-03-23 after Milestone 2 completion:

- `CredentialResolver` in `src/sandbox/proxy/http.rs` now uses ADR 006's
  dual-trait pattern. `EnvCredentialResolver` and `NoCredentialResolver`
  implement `NativeCredentialResolver` directly.
- `ChannelSecretUpdater` in `src/channels/channel.rs` now uses ADR 006's
  dual-trait pattern. `HttpChannelState` in `src/channels/http.rs` implements
  `NativeChannelSecretUpdater` directly.
- `HttpInterceptor` in `src/llm/recording.rs` now uses ADR 006's dual-trait
  pattern. `RecordingHttpInterceptor` and `ReplayingHttpInterceptor` implement
  `NativeHttpInterceptor` directly.
- `WasmToolStore` in `src/tools/wasm/storage.rs` now uses ADR 006's dual-trait
  pattern. `PostgresWasmToolStore` and `LibSqlWasmToolStore` implement
  `NativeWasmToolStore` directly.
- All seven Milestone 2 families are complete. Whole-tree footprint:
  217 matched lines for `async-trait|async_trait` in `src/`,
  150 remaining `#[async_trait]` attribute usages, all in Milestone 3/4
  families.
- All six applicable Milestone 3 families are complete
  (`NetworkPolicyDecider`, `TranscriptionProvider`, `Hook`,
  `EmbeddingProvider`, `Tunnel`, `SecretsStore`). `Observer` is sync-only
  and required no migration. Whole-tree footprint: 177 matched lines for
  `async-trait|async_trait` in `src/`, 119 remaining `#[async_trait]`
  attribute usages, all in Milestone 4 families (`Channel`, `Tool`,
  `LlmProvider`, `Database`).

Re-audit snapshot as of 2026-03-23 after Milestone 4 completion:

- All Milestone 4 families are now complete: `Tool` (64 impl blocks across
  36 files), `LlmProvider` (23 impl blocks across 15 files), `Channel` (9
  impl blocks across `src/channels/`, `src/testing/`, and `tests/support/`),
  and the `Database` family (7 sub-traits plus both PostgreSQL and libSQL
  backends across 9 impl files).
- Whole-tree footprint: 0 production `#[async_trait]` attribute usages in
  `src/`. Three doc-comment prose mentions of "async-trait" remain in
  `src/llm/CLAUDE.md`, `src/evaluation/success.rs`, and
  `src/channels/wasm/storage.rs`; none are macro invocations.
- The direct `async-trait` dependency remains in `Cargo.toml` pending the
  Milestone 5 audit and removal.

### Phase 2: Migrate concrete-only traits (batch by module)

For each module, in separate commits:

- [x] `src/channels/wasm/storage.rs`
- [x] `src/evaluation/success.rs`
- [x] Re-audit remaining candidates after the first batch lands
- [x] Stop direct concrete-only expansion after the first batch and route
  all higher-effort follow-on work through ADR 006's dyn-backed pilot
  pattern

### Phase 3: pilot ADR 006 for dyn-backed traits (optional, higher effort)

- [x] Apply ADR 006's dual-trait pattern to one narrow dyn-backed trait
  family as a proof of concept
- [x] Verify that the pilot preserves object-safe call sites while removing
  `#[async_trait]` from the trait family and its implementations
- [x] Document the pilot results and follow-on migration rules for future
  dyn-backed traits

### Phase 4: Clean up

- [x] Confirm whether `async-trait` can be removed from `[dependencies]`
- [x] Document which trait families still require it and why

Remaining required `async-trait` surfaces as of 2026-03-23 (after Milestone 4):

- No production trait families require `async-trait` for their definitions or
  impl blocks. All dyn-backed families have been migrated to the ADR 006
  dual-trait pattern.
- The direct `async-trait` dependency remains in `Cargo.toml` pending the
  Milestone 5 tree audit and explicit removal commit. Three doc-comment prose
  mentions of the crate name remain in `src/` but are not attribute usages.
- The crate is also present as a transitive dependency through crates that
  have not yet updated their own code, so `cargo tree` may still show it even
  after the direct dependency is removed from `Cargo.toml`.

## Estimated scope

Table 1. Pre-ADR-006 migration scope by async-trait category (historical).

| Category | Uses | Migratable (pre-ADR-006) | Post-ADR-006 Status |
| ---------- | ------ | ------------ | ------------------- |
| Core trait definitions | ~12 | No (dyn Trait) | **Migrated** via dual-trait pattern (ADR 006) |
| Core trait `impl` blocks | ~80 | No (must match trait) | **Migrated** via dual-trait pattern (ADR 006) |
| Confirmed safe trait definitions | 2 | **Yes** | **Completed** (Milestone 1) |
| Confirmed safe impl blocks | 3 | **Yes** | **Completed** (Milestone 1) |

See ADR 006 and Milestones 2–4 for the broad rollout that reduced production
`#[async_trait]` usage to zero. Milestone 5 will remove the dependency from
`Cargo.toml`.

Table 2. Current status of the highest-value migration buckets after the
initial safe batch and ADR 006 pilots.

| Bucket | Current status | Why it stands here | Next step |
| ---------- | ---------------- | -------------------- | ----------- |
| Concrete-only traits (`WasmChannelStore`, `SuccessEvaluator`) | Completed | No dyn-backed consumers were found, so native async traits could replace `#[async_trait]` directly. | No follow-up needed unless new trait-object usage appears. |
| Narrow dyn-backed pilots (`McpTransport`, `SettingsStore`, `SoftwareBuilder`) | Completed under ADR 006 | These families needed object-safe consumers to stay intact, so the dual-trait pattern replaced `#[async_trait]` while preserving existing dyn call sites. | Use these as the reference shape for future dyn-backed migrations. |
| Remaining dyn-backed families (`Database`, `Channel`, `Tool`, `LlmProvider`, `WasmToolStore`, and smaller internal traits) | Completed under ADR 006 | All planned families migrated via the dual-trait pattern across Milestones 2–4. Zero production `#[async_trait]` attribute usages remain in `src/`. | Remove `async-trait` from `Cargo.toml` (Milestone 5) and update governing docs. |

The original directly-migratable scope was **5 of 158 uses**. The ADR 006
broad rollout (Milestones 2–4) migrated the remaining dyn-backed families,
bringing the production `#[async_trait]` footprint to **0 of 158 uses**.
Removing the direct `async-trait` dependency from `Cargo.toml` is the
remaining action in Milestone 5.

## Risks

- **Subtle behaviour differences:** `async-trait` uses `Send`-bound futures
  by default (`#[async_trait]` implies `Send`). Native async traits do not
  add `Send` bounds automatically. If any migrated trait is used in a
  `Send`-requiring context (e.g., spawned on tokio), the migration may
  need explicit `Send` bounds added to the trait or its methods.
- **Scope drift:** The original estimate assumed a much larger concrete-only
  bucket. Future batches must be re-audited before code changes begin.
- **Incomplete classification:** A trait that appears concrete-only today
  may be used as `dyn Trait` in uncommon code paths. The audit phase must
  be thorough.

## Progress

- [x] Phase 1: Audit and classify
- [x] Phase 2: Migrate concrete-only traits
- [x] Phase 3: Pilot ADR 006 for dyn-backed traits (optional)
- [x] Phase 4: Clean up

## Progress notes

- 2026-03-20: Audited every async trait definition and corrected the
  original scope estimate. Most "internal" traits still flow through
  `dyn`/`Arc<dyn>`/`Box<dyn>` call sites and are therefore out of scope for
  native async traits without broader refactors.
- 2026-03-20: Started the first safe migration batch in
  `src/channels/wasm/storage.rs` and `src/evaluation/success.rs`, using
  return-position `impl Future + Send` in trait definitions to preserve the
  `Send` contract that `async-trait` previously supplied implicitly.
- 2026-03-21: Re-audited the remaining async traits after the first batch.
  No additional low-risk migrations were found. The next meaningful work
  item is architectural: pilot ADR 006's object-safe dual-trait pattern
  for one blocked trait family.
- 2026-03-21: ADR 006 records the
  proposed design direction for the remaining dyn-backed traits:
  `docs/adr-006-dual-trait-pattern-for-dyn-backed-async-interfaces.md`.
- 2026-03-21: ADR 006 supersedes `trait_variant` as the active Phase 3
  plan for remaining dyn-backed async traits. Any `trait_variant`
  discussion in this document is retained only as historical background.
- 2026-03-21: Piloted ADR 006 on the `McpTransport` family. The dyn-facing
  `McpTransport` trait now uses an explicit boxed-future boundary, while
  `NativeMcpTransport` keeps concrete implementations on native async
  methods. `HttpMcpTransport`, `StdioMcpTransport`, `UnixMcpTransport`,
  and the client test double migrated without changing `Arc<dyn
  McpTransport>` call sites in the client and factory layers.
- 2026-03-21: Follow-on rule from the pilot: keep the existing trait name
  on the dyn-facing surface, add a `Native*` sibling for implementations,
  and only export the sibling trait when external or cross-module
  implementors need the ergonomic path. The next candidates should be
  comparably small dyn-backed families rather than `Tool`, `LlmProvider`,
  or `Database`.
- 2026-03-21: Expanded the ADR 006 pattern to `SettingsStore` and
  `SoftwareBuilder`, the two narrow follow-on candidates named in the
  ADR. `PgBackend`, `LibSqlBackend`, and `LlmSoftwareBuilder` now use the
  native sibling traits, while existing `Arc<dyn SettingsStore>` and
  `Arc<dyn SoftwareBuilder>` call sites remain unchanged.
- 2026-03-22: Closed the clean-up phase. A fresh tree audit confirmed
  that `async-trait` is still required as a direct dependency because
  multiple dyn-backed trait families remain on the old pattern. The
  execplan now records those remaining families explicitly, instead of
  leaving Phase 4 open as if dependency removal were still plausible on
  this branch.
- 2026-03-22: Follow-up audit correction: `WasmToolStore` remains on
  `#[async_trait]` in `src/tools/wasm/storage.rs` and still flows through
  `&dyn WasmToolStore` consumers in the loader and registry code, so it
  belongs in the remaining-family list and in the broad rollout plan.
