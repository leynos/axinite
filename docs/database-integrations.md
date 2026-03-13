# Axinite database integrations

## Front matter

- **Status:** Draft implementation reference for the currently shipped
  database backends.
- **Scope:** How axinite selects, initialises, migrates, and uses PostgreSQL
  with pgvector and libSQL in this repository.
- **Primary audience:** Maintainers and operators who need to understand the
  persistence backends before changing storage behaviour or choosing a
  deployment shape.
- **Precedence:** The code in `src/db/`, `src/history/`, `src/workspace/`, and
  `migrations/` is the source of truth. `docs/configuration-guide.md` remains
  the operator reference for CLI flags and environment variables.

## 1. Design scope

Axinite treats persistence as a backend-agnostic service at most runtime call
sites, but the implementation still has two distinct integration paths:

- PostgreSQL, which remains the default backend and the richer search path
- libSQL, which supports local embedded storage, Turso-backed replicas, and
  low-friction single-user deployment

This document explains the implemented integration, not the aspirational one.
Where comments or older notes differ from the live code, this document follows
the current code paths.

## 2. Integration model

### 2.1 Backend selection

The runtime resolves the backend through `DatabaseConfig`, then connects
through `crate::db::connect_with_handles()`.

Table 1. Backend selection and startup rules.

| Concern | Current implementation |
|---------|------------------------|
| Default backend | `postgres` unless `DATABASE_BACKEND` says otherwise or bootstrap auto-detects a local libSQL file |
| Accepted backend names | `postgres`, `postgresql`, `pg`, `libsql`, `turso`, and `sqlite` |
| PostgreSQL bootstrap requirement | `DATABASE_URL` must be present |
| libSQL bootstrap requirement | `DATABASE_URL` is replaced with an internal placeholder; `LIBSQL_PATH` defaults to `~/.ironclaw/ironclaw.db` |
| libSQL remote sync requirement | `LIBSQL_URL` requires `LIBSQL_AUTH_TOKEN` |
| Auto-detection | If `DATABASE_BACKEND` is unset and `~/.ironclaw/ironclaw.db` exists, bootstrap sets `DATABASE_BACKEND=libsql` before Tokio starts |
| Feature flags | `Cargo.toml` enables both `postgres` and `libsql` by default; each backend still lives behind its own feature gate |

The main host path uses `AppBuilder::init_database()` to connect, run
migrations, migrate legacy disk settings into the database, reload
configuration from the `settings` store, attach the session manager to the
database, and schedule stale sandbox-job cleanup. The rest of the runtime then
mostly talks to `Arc<dyn Database>` rather than to concrete backend types.

### 2.2 Shared abstraction boundary

The central contract is the `Database` supertrait in `src/db/mod.rs`. It
groups narrower traits for:

- conversations and durable chat history
- agent jobs, job actions, and LLM call accounting
- sandbox job tracking
- routines and routine runs
- settings storage
- tool-failure tracking
- workspace documents, chunks, and search

This design keeps most of the host backend-agnostic, but two categories still
retain concrete backend handles:

- the encrypted secrets store
- the WASM tool and channel storage layers that want their own connections

That is why `connect_with_handles()` returns both the trait object and a
`DatabaseHandles` bundle containing either a PostgreSQL pool or a shared
libSQL database handle.

## 3. PostgreSQL and pgvector integration

### 3.1 Connection and transport model

PostgreSQL uses `deadpool-postgres` for pooling and `tokio-postgres` for
queries. `Store::new()` builds a pool from `DATABASE_URL` and
`DATABASE_POOL_SIZE`, then `crate::db::tls::create_pool()` selects either:

- plain TCP with `NoTls` when `DATABASE_SSLMODE=disable`
- a Rustls connector backed by the platform certificate store for
  `prefer` and `require`

One important implementation detail is that `prefer` and `require` do not
currently behave differently. Both paths build a TLS connector and fail if the
server rejects the handshake. The enum keeps libpq-style names, but the code
does not yet implement a true "try TLS, then retry without it" fallback.

### 3.2 Migration model

PostgreSQL migrations are embedded directly from the `migrations/` directory
through `refinery`. Every numbered SQL migration in that directory is part of
the PostgreSQL path.

The initial migration, `V1__initial.sql`, creates the full schema and enables
the `vector` extension up front:

- conversation and job tables
- settings and routine state
- workspace document and chunk tables
- `tsvector` full-text search support
- pgvector support for embeddings

The onboarding wizard validates two PostgreSQL prerequisites before it accepts
a connection string:

1. PostgreSQL major version 15 or newer
1. `pgvector` available through `pg_available_extensions`

That matches the current operator expectation in the code: PostgreSQL is not
just "any SQL server", it is specifically a PostgreSQL deployment with
pgvector installed.

### 3.3 Workspace search path

The PostgreSQL workspace repository is the richer search implementation.

Table 2. PostgreSQL workspace search components.

| Component | Implementation |
|-----------|----------------|
| Document store | `memory_documents` table |
| Chunk store | `memory_chunks` table |
| Full-text search | Generated `content_tsv` column plus a GIN index |
| Semantic search | pgvector `embedding` column queried with cosine distance |
| Fusion strategy | Reciprocal Rank Fusion (RRF) in Rust after separate FTS and vector queries |

There is, however, an important current-state caveat. The initial schema used
`VECTOR(1536)` plus an HNSW index. Migration `V9__flexible_embedding_dimension.sql`
changes the embedding column to unconstrained `vector` so different embedding
models can coexist, and it drops the HNSW index because pgvector indexes need
a fixed dimension. The current PostgreSQL path therefore still performs vector
search, but it does so without the old approximate index.

That means the live PostgreSQL behaviour is:

- keyword search through `ts_rank_cd` and `plainto_tsquery('english', ...)`
- exact cosine-distance ordering through pgvector
- RRF across the two result streams

### 3.4 Satellite stores on PostgreSQL

PostgreSQL-specific integrations do not stop at the main `Database` trait.
The backend pool is also reused for:

- `PostgresSecretsStore`
- `PostgresWasmToolStore`
- the older `history::Store` and `workspace::Repository` wrappers that the
  backend still delegates to internally

This is why the PostgreSQL backend owns both a `Store` and a `Repository`:
the unified trait layer is intentionally thin over the earlier backend-native
implementations.

## 4. libSQL integration

### 4.1 Deployment modes

libSQL uses Turso's SQLite fork and supports three distinct modes in the code:

- local file-backed embedded storage via `LibSqlBackend::new_local()`
- remote replica mode via `LibSqlBackend::new_remote_replica()`
- in-memory mode via `LibSqlBackend::new_memory()` for tests

Remote replica mode still uses a local file path. The backend opens an
embedded replica at `LIBSQL_PATH`, then synchronises it with `LIBSQL_URL`
using `LIBSQL_AUTH_TOKEN`.

### 4.2 Connection model

libSQL does not use a pool. `LibSqlBackend::connect()` opens a fresh
connection per operation and applies two pragmatic safeguards:

- `PRAGMA busy_timeout = 5000` on every connection so transient writer
  contention waits instead of failing immediately
- a small retry loop when connection creation briefly reports "unable to open
  database file"

During migration the backend also sets `PRAGMA journal_mode=WAL`, which
persists in the database file and improves reader and writer coexistence for
future connections.

This per-operation model is also reused by libSQL-backed satellite stores such
as the secrets store and the WASM storage layer. They share the underlying
`Arc<libsql::Database>`, but create their own short-lived connections.

### 4.3 Migration model

libSQL does not run the PostgreSQL SQL files directly. Instead it uses a
two-layer approach:

1. a consolidated SQLite-compatible bootstrap schema from
   `migrations/libsql_schema.sql`
1. incremental migrations from `src/db/libsql_migrations.rs`, tracked in the
   `_migrations` table

The bootstrap schema translates PostgreSQL concepts into SQLite or libSQL
equivalents. Examples include:

- `UUID` to `TEXT`
- `TIMESTAMPTZ` to RFC 3339 `TEXT`
- `JSONB` to JSON-encoded `TEXT`
- `NUMERIC` to `TEXT` so `rust_decimal` precision survives round-trips
- `TSVECTOR` and related functions to FTS5 virtual tables and triggers
- pgvector columns to `BLOB`

This design exists because the PostgreSQL migrations rely on types, indexes,
and helper functions that SQLite cannot execute directly.

The incremental libSQL path currently covers:

- V9 flexible embedding dimensions
- V10 WIT-version default rebuilds for WASM tables
- V12 token-budget columns on `agent_jobs`

Each incremental migration is recorded in `_migrations`, and most are wrapped
in a transaction. The exceptional V10 rebuild runs through a dedicated
non-transactional helper because it needs SQLite PRAGMA changes and its own
explicit `BEGIN IMMEDIATE` block.

### 4.4 Workspace search path

libSQL mirrors the same workspace concepts as PostgreSQL, but the implemented
search path is narrower.

Table 3. libSQL workspace search components.

| Component | Implementation |
|-----------|----------------|
| Document store | `memory_documents` table |
| Chunk store | `memory_chunks` table with `embedding BLOB` |
| Full-text search | `memory_chunks_fts` FTS5 virtual table plus maintenance triggers |
| Semantic search | Best-effort `vector_top_k(...)` query when a compatible vector index exists |
| Fusion strategy | RRF in Rust, same as PostgreSQL |

The current implementation quirk is important:

- the libSQL schema and V9 migration comments describe a brute-force vector
  fallback after flexible dimensions remove the fixed-dimension index
- the live code in `src/db/libsql/workspace.rs` does not implement that
  brute-force fallback
- instead it attempts `vector_top_k('idx_memory_chunks_embedding', ...)`
  and, when that query fails as expected after V9 drops the index, it logs a
  debug message and returns no vector results

In practical terms, a migrated or freshly bootstrapped libSQL workspace
currently behaves as:

- FTS5 keyword search always available
- vector results only when a compatible vector index exists
- FTS-only search after the normal flexible-dimension migration path

That is the most significant behavioural gap between the two backends in the
current code.

### 4.5 Satellite stores on libSQL

libSQL also feeds the wider persistence surface beyond the main trait object.
The shared database handle is reused by:

- `LibSqlSecretsStore`
- `LibSqlWasmToolStore`
- the libSQL implementations of conversation, job, routine, sandbox, settings,
  and workspace persistence modules

The key pattern is "shared database, fresh connection per operation". The code
explicitly avoids passing a long-lived `Connection` into these satellite
stores.

## 5. Shared persistence consumers

The backend choice affects more than tables and queries. Several runtime
subsystems are layered on top of the database abstraction.

### 5.1 Configuration and legacy migration

After the database connects, startup calls `migrate_disk_to_db()` to import
legacy on-disk state into the `settings` table. That routine migrates:

- legacy `settings.json`
- `mcp-servers.json`
- `session.json` under the `nearai.session_token` setting key

The host then rebuilds configuration from the `settings` store, optional TOML,
and the environment. This means the database backend is not just a data sink:
it becomes part of the configuration overlay after bootstrap succeeds.

### 5.2 Session persistence

The NEAR AI session manager persists session state to disk, but when a
database is attached it also writes the session token into the settings table.
On load, the session manager prefers the database-backed copy over the file on
disk. That behaviour is shared across both backends because it sits above the
`SettingsStore` trait.

### 5.3 Workspace memory

Workspace memory is created only when a database backend is available. In
no-database mode the host simply skips workspace construction, memory tools,
and embedding backfill.

When a backend is present, the workspace layer is the same logical product
surface on both backends:

- filesystem-like documents
- chunking
- optional embeddings
- search
- seeded runbooks and bootstrap files
- hygiene passes and embedding backfill

The runtime difference is in the concrete SQL and search fidelity, not in the
high-level API.

## 6. Operational differences and decision points

Table 4. Current backend comparison.

| Concern | PostgreSQL with pgvector | libSQL |
|---------|--------------------------|--------|
| Default on this branch | Yes | No, unless bootstrap auto-detects `~/.ironclaw/ironclaw.db` |
| External service needed | Yes | No for local mode; optional for Turso replica mode |
| Connection strategy | Shared async pool | Fresh connection per operation |
| Migration engine | `refinery` over numbered SQL files | Consolidated schema plus `_migrations`-tracked incremental Rust-side runner |
| Secrets and WASM satellite stores | Reuse cloned pool handles | Reuse shared database handle, then open fresh connections |
| Keyword search | PostgreSQL `tsvector` plus GIN | FTS5 virtual table plus triggers |
| Vector search | pgvector cosine distance, now without the old HNSW index after V9 | Best-effort only; current migrated path is effectively FTS-only |
| Best fit | Full default deployment with richer search parity | Embedded, local-first, or low-ops deployment where external PostgreSQL is undesirable |

### 6.1 When PostgreSQL is the safer choice

Choose PostgreSQL when:

- full hybrid workspace search quality matters
- the deployment already has PostgreSQL 15+ with pgvector available
- query behaviour should match the default and most-tested path as closely as
  possible

### 6.2 When libSQL is the better fit

Choose libSQL when:

- a single-file embedded database is operationally simpler
- the deployment is local-first or edge-style
- Turso replica mode is desirable, but a full PostgreSQL service is not

The main caveat is the current workspace-search trade-off. libSQL is not
merely "the same database API with a different wire protocol". In the current
implementation it is a simpler persistence backend with weaker semantic-search
behaviour after the flexible-dimension migration path.

## 7. Current implementation caveats

1. `DATABASE_SSLMODE=prefer` currently behaves like `require` in practice,
   because the PostgreSQL TLS helper does not retry without TLS after a failed
   handshake.
1. PostgreSQL still supports vector search after V9, but no longer through the
   old fixed-dimension HNSW index.
1. libSQL migration and schema comments still describe a brute-force vector
   fallback that the current code does not implement.
1. Workspace memory and memory tools are absent in `--no-db` mode because the
   host does not build a workspace without a database.
