# Migrate from async-trait to native async traits

**Branch:** (to be created from `build-time`)
**Date:** 2026-03-15
**Status:** Plan ready; not yet started
**Estimated impact:** Reduced proc-macro expansion overhead (158 uses
across 74 files)

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
  methods without manual boxing or the `trait_variant` crate.
- The migration must be incremental — it is not feasible or desirable to
  change all 158 uses in a single commit.
- Each batch of changes must pass `make all`.

## Trait Classification

### Core extensibility traits (used as `dyn Trait` — KEEP `async-trait`)

These traits are used as trait objects throughout the codebase and cannot
trivially migrate to native async traits:

- `Database` (`src/db/mod.rs`) — 10 `dyn` references
- `Channel` (`src/channels/channel.rs`) — used via `dyn Channel`
- `Tool` (`src/tools/tool.rs`) — used via `dyn Tool`
- `LlmProvider` (`src/llm/provider.rs`) — 19 `dyn` references in
  `src/llm/mod.rs` alone
- `SuccessEvaluator` (`src/evaluation/success.rs`)
- `EmbeddingProvider` (`src/workspace/embeddings.rs`)
- `NetworkPolicyDecider` (`src/sandbox/proxy/policy.rs`)
- `Hook` (`src/hooks/hook.rs`)
- `Observer` (`src/observability/traits.rs`)
- `Tunnel` (`src/tunnel/mod.rs`)
- `SecretStore` (`src/secrets/store.rs`)
- `Transcriber` (`src/transcription/mod.rs`)

### Concrete-only traits (safe to migrate)

Traits that are only used with concrete types (impl blocks, generics with
`impl Trait`, never `dyn Trait`) can be migrated. These include:

- Internal implementation traits in `src/tools/builtin/*.rs`
- Helper traits in `src/tools/mcp/*.rs` (transports)
- Internal traits in `src/tools/wasm/*.rs`
- Concrete implementations of the above core traits (the `impl` blocks)
- Helper/adapter traits in `src/llm/` (failover, circuit breaker,
  recording, etc.)

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
4. Optionally, evaluate `trait_variant` crate for providing both
   object-safe and non-object-safe variants of core traits in the future.

### Phase 1: Audit and classify every `#[async_trait]` use

- [ ] For each trait with `#[async_trait]`, determine whether it is ever
  used as `dyn Trait`
- [ ] Produce a spreadsheet/table of: trait name, file, definition or impl,
  dyn-used (yes/no), migratable (yes/no)
- [ ] Identify the subset that can be migrated

### Phase 2: Migrate concrete-only traits (batch by module)

For each module, in separate commits:

- [ ] `src/tools/mcp/` transports (stdio, unix, http) — likely 3–5 uses
- [ ] `src/tools/wasm/` internal traits — likely 3–5 uses
- [ ] `src/tools/builtin/` helper traits — likely 10–15 uses
- [ ] `src/llm/` internal traits (recording, response_cache, etc.) —
  likely 5–10 uses
- [ ] `src/worker/` traits — likely 2–3 uses
- [ ] Remaining scattered uses

### Phase 3: Evaluate core trait migration (optional, higher effort)

- [ ] Assess whether `trait_variant` or manual `-> Pin<Box<dyn Future>>`
  return types would allow removing `async-trait` from core traits
- [ ] If viable, migrate one core trait as a proof of concept
- [ ] Document the pattern for future migrations

### Phase 4: Clean up

- [ ] If all uses are removed, remove `async-trait` from `[dependencies]`
- [ ] If some uses remain, document which traits still require it and why

## Estimated Scope

Table 1. Migration scope by async-trait category.

| Category | Uses | Migratable |
|----------|------|------------|
| Core trait definitions | ~12 | No (dyn Trait) |
| Core trait `impl` blocks | ~80 | No (must match trait) |
| Concrete-only traits + impls | ~66 | **Yes** |

Roughly **66 of 158 uses** (~42%) can be migrated. The remaining 92 uses
are tied to core extensibility traits that require object safety.

## Risks

- **Subtle behaviour differences:** `async-trait` uses `Send`-bound futures
  by default (`#[async_trait]` implies `Send`). Native async traits do not
  add `Send` bounds automatically. If any migrated trait is used in a
  `Send`-requiring context (e.g., spawned on tokio), the migration may
  need explicit `Send` bounds added to the trait or its methods.
- **Large diff:** Even the concrete-only migration touches ~66 locations
  across many files. Breaking this into module-scoped commits is essential.
- **Incomplete classification:** A trait that appears concrete-only today
  may be used as `dyn Trait` in uncommon code paths. The audit phase must
  be thorough.

## Progress

- [ ] Phase 1: Audit and classify
- [ ] Phase 2: Migrate concrete-only traits
- [ ] Phase 3: Evaluate core trait migration (optional)
- [ ] Phase 4: Clean up
