# Migrate from async-trait to native async traits

**Branch:** `migrate-async-trait`
**Date:** 2026-03-15
**Status:** In progress
**Estimated impact:** Reduced proc-macro expansion overhead for the small
subset of async traits that are not used as trait objects

## Big Picture

The `async-trait` proc macro is used 158 times across 74 source files.
Each use generates boxing and dynamic dispatch boilerplate at compile time.
Since the project targets Rust 1.92 (which supports native `async fn` in
traits, stabilized in Rust 1.75), most uses can be migrated to native
syntax, eliminating the proc-macro expansion overhead and reducing compile
times.

However, native async traits are **not object-safe** — it is not possible
to write `dyn MyAsyncTrait` without boxing the future. This project uses
`dyn Trait` extensively (246 occurrences across 65 files for the core
extensibility traits). This means the migration is **partial**: traits used
as trait objects must either remain with `async-trait` or adopt a manual
boxing pattern.

## Constraints

- Rust edition 2024, minimum version 1.92.
- Traits used as `dyn Trait` (boxed trait objects) cannot use native async
  methods directly. ADR 006 adopts a dual-trait pattern for those surfaces.
- The migration must be incremental — it is not feasible or desirable to
  change all 158 uses in a single commit.
- Each batch of changes must pass `make all`.

## Trait Classification

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

## Migration Strategy

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
- [x] Produce a table of: trait name, file, dyn-used (yes/no),
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

### Phase 2: Migrate concrete-only traits (batch by module)

For each module, in separate commits:

- [x] `src/channels/wasm/storage.rs`
- [x] `src/evaluation/success.rs`
- [x] Re-audit remaining candidates after the first batch lands
- [ ] Only expand into higher-effort modules once trait-object usage has
  been eliminated

### Phase 3: Pilot ADR 006 for dyn-backed traits (optional, higher effort)

- [ ] Apply ADR 006's dual-trait pattern to one narrow dyn-backed trait
  family as a proof of concept
- [ ] Verify that the pilot preserves object-safe call sites while removing
  `#[async_trait]` from the trait family and its implementations
- [ ] Document the pilot results and follow-on migration rules for future
  dyn-backed traits

### Phase 4: Clean up

- [ ] If all uses are removed, remove `async-trait` from `[dependencies]`
- [ ] If some uses remain, document which traits still require it and why

## Estimated Scope

Table 1. Migration scope by async-trait category.

| Category | Uses | Migratable |
| ---------- | ------ | ------------ |
| Core trait definitions | ~12 | No (dyn Trait) |
| Core trait `impl` blocks | ~80 | No (must match trait) |
| Confirmed safe trait definitions | 2 | **Yes** |
| Confirmed safe impl blocks | 3 | **Yes** |

The currently verified scope is **5 of 158 uses**. More may become
migratable later, but only after removing trait-object usage or adopting a
different object-safety pattern.

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
- [ ] Phase 2: Migrate concrete-only traits
- [ ] Phase 3: Pilot ADR 006 for dyn-backed traits (optional)
- [ ] Phase 4: Clean up

## Progress Notes

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
- 2026-03-21: Architectural decision record (ADR) 006 records the
  proposed design direction for the remaining dyn-backed traits:
  `docs/adr-006-dual-trait-pattern-for-dyn-backed-async-interfaces.md`.
- 2026-03-21: ADR 006 supersedes `trait_variant` as the active Phase 3
  plan for remaining dyn-backed async traits. Any `trait_variant`
  discussion in this document is retained only as historical background.
