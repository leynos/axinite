# Build time investigation and recommendations

**Branch:** `build-time`
**Date:** 2026-03-15
**Status:** Investigation complete; recommendations ready for review

## Big Picture

Investigate slow build/test cycles in the ironclaw repository and identify
actionable improvements to reduce overall build and test time.

## Constraints

- Do not use `/tmp` as a build target (only 32 GB).
- Changes must not break continuous integration (CI) or existing developer
  workflows.
- Recommendations must be ranked by effort/impact ratio.

## Baseline Measurements

**Environment:** Rust 1.92.0, 32 cores, mold linker configured, no sccache.

Table 1. Baseline build and test metrics.

| Metric | Value |
|--------|-------|
| Total unique crates | 810 |
| Source lines (src/) | 174,101 |
| Integration test binaries | 43 |
| Test count | 3,209 |
| Clean `make all` | **21 min 31 s** |
| Incremental `make all` (one file touched) | **9 min 16 s** |

### Clean Build Phase Breakdown

Table 2. Clean build phase breakdown.

| Phase | Wall time | Notes |
|-------|-----------|-------|
| `check-fmt` | 2.4 s | Negligible |
| `typecheck` | 6 min 27 s | First dep compilation + 4 cargo checks |
| `lint` | 5 min 44 s | 4 cargo clippy (deps cached from typecheck) |
| `test` | 9 min 18 s | WebAssembly (WASM) (18 s) + compile (8 m 11 s) + run (31 s) |

### Incremental Build Breakdown

Table 3. Incremental build breakdown.

| Phase | Wall time | Notes |
|-------|-----------|-------|
| `check-fmt` | ~1 s | |
| `typecheck` (3 combos + GitHub) | ~51 s | ironclaw recompiled 3 times |
| `lint` (3 combos + GitHub) | ~108 s | ironclaw recompiled 3 more times |
| `test` compilation | **6 min 05 s** | Re-links 43 test binaries |
| `test` execution | 21 s | Fast |
| **Total** | **~9 min 16 s** | |

### Heaviest Crates (clean build)

Table 4. Heaviest crates by compile time.

| Crate | Compile time | Notes |
|-------|-------------|-------|
| ironclaw (lib) | 133.0 s | The project itself |
| wasmtime-wasi | 17.8 s | WASM runtime |
| rig-core | 16.6 s | Large language model (LLM) framework |
| wasmtime | 16.3 s | |
| zbus | 13.7 s | Linux D-Bus |
| ironclaw (bin) | 11.3 s | Binary linking |
| wasmtime-cranelift | 8.0 s | Compiler backend |
| bollard-stubs | 7.9 s | Docker API |
| libsql | 6.8 s | |
| tokio | 6.6 s | |

## Root Causes

### 1. Redundant typecheck/lint passes (incremental: ~160 s wasted)

`make all` runs `cargo check` with 3 feature combos, then `cargo clippy`
with the same 3 feature combos. Clippy is a strict superset of `cargo
check` — it performs all type-checking plus lint analysis. Running check
first only warms the dependency cache, which clippy does anyway. Each pass
recompiles the ironclaw crate itself (different compiler driver fingerprint).

CI already skips `cargo check` and runs only clippy. The Makefile is
behind.

### 2. Test binary relinking (incremental: ~365 s)

43 separate integration test binaries exist in `tests/`. Each top-level
`.rs` file compiles as an independent binary that links against the entire
ironclaw crate and all dev-dependencies. 21 of these import a shared
`mod support;` module that is recompiled into each binary. A single-file
change triggers 43 relink operations.

### 3. Massive dependency tree with heavy duplication

810 unique crates. libsql alone pulls in a parallel legacy HTTP/gRPC stack,
causing ~15 major version duplicates:

- axum 0.6 + 0.8
- hyper 0.14 + 1.8
- h2 0.3 + 0.4
- tower 0.4 + 0.5
- tower-http 0.4 + 0.6
- rustls 0.22 + 0.23
- http 0.2 + 1.4
- thiserror 1.x + 2.x
- hashbrown (4 versions!)
- getrandom (3 versions!)

### 4. Always-compiled heavy dependencies not feature-gated

| Dependency | Transitive crates | Could be optional |
|------------|-------------------|-------------------|
| wasmtime + wasmtime-wasi | ~300 | Yes (`wasm` feature) |
| bollard | 156 | Yes (`docker` feature) |
| pdf-extract | 50 | Yes (`pdf` feature) |
| rustyline + crossterm + termimad | ~40 | Yes (`cli` feature) |

### 5. Dual Transport Layer Security (TLS) stacks compiled

reqwest is configured with `rustls-tls-native-roots` and
`default-features = false`, but rig-core pulls in reqwest with default
features, causing both native-tls (openssl-sys C compilation) and rustls
to be compiled.

### 6. Monolithic crate structure

174K lines in a single compilation unit. The three largest modules (tools,
channels, agent) are too tightly coupled for cost-effective splitting, but
llm (19K lines) is relatively self-contained.

### 7. 261 async-trait proc macro uses

`async-trait` is used 261 times across the codebase. Each use generates
significant expansion code. The project targets Rust 1.92, which supports
native async traits — migration would eliminate this overhead.

## Recommendations

### Tier 1 — Quick Wins (config/Makefile changes, no code changes)

#### 1.1 Remove `typecheck` from `make all`

**Impact:** Saves ~51 s incremental, ~6.5 min clean (on first run).
**Effort:** One-line Makefile change.

Clippy is a strict superset of `cargo check`. CI already runs only clippy.
Change `all: check-fmt typecheck lint test` to `all: check-fmt lint test`.
Retain `typecheck` as a standalone target for quick smoke-checks.

#### 1.2 Set `debug = "line-tables-only"` for dependencies

**Impact:** Reduces debug info size, speeds up linking (affects all 43 test
binary links).
**Effort:** Add 2 lines to `Cargo.toml`.

Full debug info for dependencies is rarely needed for day-to-day
development. Line tables are sufficient for backtraces. Scoping the
override to `[profile.dev.package."*"]` preserves full debug info for
workspace members (ironclaw itself), where diagnostics matter most.

```toml
[profile.dev.package."*"]
debug = "line-tables-only"
```

#### 1.3 Set `split-debuginfo = "unpacked"` for dependencies

**Impact:** Avoids `dsymutil` on macOS and may reduce link time. On Linux
with mold, the effect is smaller but still positive.
**Effort:** 1 line in `Cargo.toml`.

```toml
[profile.dev.package."*"]
split-debuginfo = "unpacked"
```

#### 1.4 Run `check-fmt` and `build-github-tool-wasm` in parallel with lint

**Impact:** Saves ~18 s (WASM build) on clean, ~1 s incremental. Marginal
but free.
**Effort:** Restructure Makefile dependencies to enable `make -j`.

```makefile
all: check-fmt lint test

# test depends on WASM build, but WASM can start early
test: build-github-tool-wasm
	$(NEXTEST) run --workspace $(TEST_FEATURES)
	$(CARGO) test --manifest-path $(GITHUB_TOOL_MANIFEST)
```

#### 1.5 Install and configure sccache

**Impact:** Dramatically speeds up clean builds after branch switches or
`cargo clean`. Deps that haven't changed are fetched from cache rather than
recompiled.
**Effort:** Install sccache, configure the environment variable:

| Variable name | Meaning | Default or rule |
|---------------|---------|-----------------|
| `RUSTC_WRAPPER` | A `rustc` wrapper (for example, `sccache`) used by Cargo. | Unset by default; set to the `sccache` path in `.cargo/config.toml` or via the shell environment to enable compiler caching. |

### Tier 2 — Medium Effort (test restructuring, feature gating)

#### 2.1 Consolidate integration test binaries

**Impact:** HIGH — test relinking is the dominant incremental bottleneck
(6 min 05 s). Reducing 43 binaries to ~8–10 could save 3–4 minutes.
**Effort:** MEDIUM — group related test files into modules within fewer
top-level test binaries.

Currently 43 `.rs` files in `tests/` each produce a separate binary. Group
related tests:

- Merge `agent_*` tests into `tests/agent.rs` with `mod` submodules
- Merge `tools_*` tests into `tests/tools.rs`
- Merge `channels_*` tests into `tests/channels.rs`
- Merge `db_*` tests into `tests/db.rs`
- Keep `tests/support/` as a shared module

This reduces link operations from 43 to ~8–10 while maintaining the same
test coverage and organization.

#### 2.2 Feature-gate wasmtime (`wasm` feature)

**Impact:** ~300 crates removed from the build when feature is off. Saves
~42 s on clean builds for developers not working on WASM tools.
**Effort:** MEDIUM — add feature flag, gate `src/tools/wasm/` behind it,
add to default features.

#### 2.3 Feature-gate bollard (`docker` feature)

**Impact:** ~156 crates removed when off.
**Effort:** MEDIUM — similar to wasmtime gating.

#### 2.4 Feature-gate pdf-extract (`pdf` feature)

**Impact:** ~50 crates removed when off.
**Effort:** LOW–MEDIUM.

#### 2.5 Remove wasmparser direct dependency

**Impact:** Eliminates one duplicate version (0.220 vs wasmtime's 0.221).
**Effort:** LOW — use wasmtime's re-exported wasmparser or update to 0.221.

### Tier 3 — Larger Refactors

#### 3.1 Migrate from async-trait to native async traits

**Impact:** MEDIUM — eliminates 261 proc-macro expansions, reducing
compilation and expansion overhead.
**Effort:** HIGH — touching 261 call sites. Can be done incrementally.
Rust 1.92 supports `async fn` in traits natively. Main limitation: native
async traits are not object-safe without `#[trait_variant]` or boxing, so
some uses may need to remain as `async-trait` where `dyn Trait` is used.

#### 3.2 Extract `ironclaw-types` crate

**Impact:** MEDIUM — shared traits and domain types compile once and in
parallel with the main crate.
**Effort:** MEDIUM — extract `LlmProvider`, `Database`, `Channel`, `Tool`,
and common domain types.

#### 3.3 Extract `ironclaw-llm` crate

**Impact:** MEDIUM — 19K lines compile in parallel with main crate. Most
self-contained large module.
**Effort:** MEDIUM — relatively clean boundaries.

#### 3.4 Fix reqwest dual-TLS compilation

**Impact:** LOW–MEDIUM — removes openssl-sys C compilation.
**Effort:** LOW — investigate whether rig-core can be configured to not
pull in reqwest defaults, or unify TLS backend selection.

### Not Recommended

- **Splitting tools/, channels/, or agent/ into crates** — too tightly
  coupled; refactoring cost far exceeds build time savings.
- **Custom codegen-units for dev profile** — already defaults to 256, which
  is fine.
- **Reducing Cargo parallelism** — the bottleneck is critical-path serial
  crates, not over-subscription.

## Progress

- [x] Profile full `make all` build and identify time sinks
- [x] Analyse dependency tree for heavy crates and compilation bottlenecks
- [x] Audit Makefile pipeline for redundant compilation work
- [x] Investigate workspace structure and crate splitting opportunities
- [x] Synthesize findings into actionable recommendations
- [x] Implement Tier 1 quick wins (1.1, 1.2, 1.3)
- [x] Write exec plans for Tier 2/3 changes:
  - `consolidate-test-binaries.md` (2.1)
  - `feature-gate-wasmtime.md` (2.2)
  - `feature-gate-bollard.md` (2.3)
  - `migrate-async-trait.md` (3.1)
- [ ] Implement Tier 2 changes (pending approval)
- [ ] Implement Tier 3 changes (pending approval)

## Lessons Learned

- `cargo clippy` is a strict superset of `cargo check` — running both is
  redundant (CI already knew this; the Makefile didn't).
- Integration test binaries are a major hidden build cost in Rust projects.
  Each top-level `.rs` file in `tests/` becomes its own binary with its own
  link step. Consolidation has outsized impact.
- libsql's dependency on legacy hyper/axum/tower/rustls versions causes
  massive version duplication, but this is upstream and not actionable
  without switching databases.
- Feature-gating heavy optional dependencies (wasmtime, bollard, pdf) is a
  standard Rust practice that this project hasn't fully adopted.
