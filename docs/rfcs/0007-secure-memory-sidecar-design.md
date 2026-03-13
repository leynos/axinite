# RFC 0007: Secure memory sidecar design for axinite

## Preamble

- **RFC number:** 0007
- **Status:** Proposed
- **Created:** 2026-03-13

## Executive summary

Axinite’s IronClaw already has a coherent, security-first architecture and a functioning “workspace & memory system” built on PostgreSQL tables (`memory_documents`, `memory_chunks`) and hybrid retrieval (full-text + vector) fused via Reciprocal Rank Fusion (RRF). citeturn4view0turn4view1turn6view1turn15view2 That existing system is valuable (especially for “explicit write it down” durable memory), but it is not structured as an event-driven, consolidation-capable, entity/fact memory layer. citeturn4view1turn5view0

This report proposes a **security-minded memory sidecar** (“memoryd”) that:

- **Adds a transactional outbox** in PostgreSQL, populated by IronClaw at key persistence boundaries (conversation messages; workspace document mutations), with explicit idempotency and retention. citeturn14view3turn15view2turn11view0  
- Runs a **local-only Rust sidecar** that consumes outbox events, performs extraction with **Ollama** structured outputs + embeddings, stores retrieval vectors in **Qdrant**, and stores normalized facts/relations in **Oxigraph** (embedded; no SPARQL HTTP server). citeturn29search1turn29search2turn33search1turn28search0turn28search1  
- Uses a **capability-token enforcement model** over a **Unix Domain Socket (UDS)** RPC surface with narrow, auditable scopes; tokens are minted by IronClaw per job/workspace and validated by memoryd. The design intentionally minimises network surfaces (UDS for memoryd; no Oxigraph network exposure; Qdrant/Ollama bound to loopback with keys and OS controls). citeturn7view0turn17view0turn27search0turn29search0  
- Delegates consolidation and heavyweight reconciliation to **Apalis** workers (embedded into memoryd), enabling retries, timeouts, concurrency limits, and a reliable Postgres-backed queue. citeturn30search0turn30search1  

Open choices (explicit “no constraint” per your request):

- Embedding model(s) (must be consistent for ingest vs recall), extraction model, and whether to add sparse vectors; these remain configurable. Axinite already exposes a unified embeddings config supporting OpenAI / NEAR AI / Ollama and model-dependent vector dimensions. citeturn20view0turn20view4  
- Whether to structure Qdrant as collection-per-workspace (strong isolation, easy purge) or shared collections filtered by payload (fewer collections, more reliance on correct filters). This report recommends **collection-per-workspace** for security and deletion correctness.

## Baseline architecture and constraints from Axinite

IronClaw’s README and internal development guide describe an architecture built around: multi-channel inputs → agent loop → tools (built-in, MCP, WASM) → PostgreSQL persistence; with explicit defence-in-depth (WASM sandbox with capabilities, prompt-injection defences, secret protection). citeturn4view0turn3view0

The existing workspace subsystem:

- Stores documents in `memory_documents` and chunks in `memory_chunks`. citeturn6view0turn15view0turn15view2  
- Implements hybrid search by combining PostgreSQL FTS and pgvector cosine similarity with an RRF fusion function. citeturn4view1turn6view0turn6view1  
- Reindexes chunks after writes/append (so “durable memory” stays searchable). citeturn5view2turn6view0  
- Already constrains risky memory mutations: built-in `memory_write` blocks overwriting “identity files” loaded into the system prompt to mitigate prompt-injection persistence attacks. citeturn8view0  

Security primitives we can reuse:

- A job-scoped bearer-token store exists for worker↔orchestrator HTTP authentication (random 32-byte tokens, constant-time compare, per-job scoping). citeturn7view0  
- A capability system exists for WASM tools where *all permissions are opt-in* (workspace read prefixes, HTTP allowlists, tool invocation aliases, secret existence checks). citeturn17view0turn7view1  
- A leak detector exists that can redact or block outputs containing secret-like patterns (Aho–Corasick + regex; actions include Block/Redact/Warn). citeturn36view0turn36view1  

Design constraint: Axinite’s Postgres schema includes `conversation_messages`, `memory_documents`, and `memory_chunks`, with indexes (e.g. `idx_memory_chunks_tsv`, HNSW on embeddings in V1; later dropped for variable-dimension embeddings). citeturn14view3turn15view2turn16view0 The sidecar must not assume a fixed embedding dimension in the primary DB, and should treat embedding dimension as a configuration contract. citeturn20view2turn16view0  

## Required IronClaw code changes

This section lists concrete changes to Axinite’s IronClaw codebase to integrate a secure memory sidecar while keeping surfaces small and enforcing least privilege.

**Add a memory-sidecar config block**

Introduce `MemorySidecarConfig` in `src/config/` and add it into the top-level `Config` struct (see existing pattern in `src/config/mod.rs`). citeturn19view0

Key fields:

- `enabled: bool`
- `mode: Disabled|Shadow|Active`
- `uds_path: PathBuf` (default under `~/.ironclaw/run/memoryd.sock`)
- `cap_issuer: String` (e.g. `ironclaw`)
- `cap_audience: String` (e.g. `memoryd`)
- `cap_ttl_seconds: u32` (short-lived, e.g. 60–300s depending on method)
- `outbox_poll_ms: u32` (if IronClaw also runs a notifier; optional)
- `qdrant_url`, `qdrant_api_key_secret_ref` (if IronClaw needs to provision; otherwise memoryd owns)
- `ollama_base_url` (already present globally for embeddings; reuse) citeturn20view1  

**Capability minting module**

Add `src/memory_sidecar/capability.rs` in IronClaw to mint capability tokens for memoryd calls. Use the same philosophy as WASM capabilities (explicit, opt-in, narrow) but expressed as signed tokens. citeturn17view0  

Minimum minting call sites:

- When executing built-in memory tools (current `memory_search`, `memory_read`, `memory_write`) to call memoryd in **Shadow/Active** modes. citeturn8view0turn23view0  
- When persisting conversation messages (into `conversation_messages`) to produce outbox events, and optionally when persisting workspace docs to produce outbox events. citeturn14view3turn15view0  

**UDS client plumbing**

Add a `MemorydClient` with:

- `connect(uds_path) -> UnixStream`
- `call(method, request) -> response` with length-delimited framing and timeouts.

Keep it in the orchestrator domain (do not expose it to WASM; do not allow arbitrary untrusted tools to talk to it). This matches the established separation between orchestrator-only tools vs container tools. citeturn17view2  

**Dual-write hooks**

Implement dual-write into **Postgres outbox** at these boundaries:

- **Conversation writes**: whenever IronClaw inserts into `conversation_messages`, insert an outbox event in the same transaction. citeturn14view3turn11view0  
- **Workspace document mutations**: whenever `memory_documents` changes materially (insert/update/delete), insert an outbox event. The existing repository already centralises document writes (`update_document`, delete, create). citeturn6view0turn5view2  

Strong recommendation: implement outbox writes **inside the same DB transaction** as the primary write to avoid “message without state” or “state without message” failure modes.

**Mode gating**

- **Disabled**: do nothing (no outbox writes; no memoryd calls).
- **Shadow**: write outbox + call memoryd for ingestion, but do not use memoryd results for user-visible answers (log only and collect metrics).
- **Active**: memory tools call memoryd for recall (and may still fall back to workspace RRF search).

This integrates cleanly with `AppBuilder::init_tools()` where tools and workspace are registered once DB exists. citeturn23view0  

## Postgres outbox schema and event types

Axinite’s V1 schema already defines the core persistence tables (`conversations`, `conversation_messages`, workspace tables). citeturn14view3turn15view2 The outbox adds a durable, ordered stream of events for memory projection and consolidation.

### Migration SQL

Create a new migration (e.g. `V13__memory_outbox.sql`) alongside existing refinery migrations. citeturn11view0turn12view1

```sql
-- V13__memory_outbox.sql
-- Transactional outbox for memory sidecar (memoryd)

CREATE TABLE IF NOT EXISTS memory_outbox (
  outbox_id       BIGSERIAL PRIMARY KEY,
  event_id        UUID NOT NULL,
  dedupe_key      TEXT NOT NULL,
  event_type      TEXT NOT NULL,

  -- Routing / tenancy
  workspace_id    TEXT NOT NULL,          -- IronClaw uses user_id like "default"
  agent_id        UUID NULL,
  conversation_id UUID NULL,

  -- Causality / audit
  job_id          UUID NULL,
  channel         TEXT NULL,              -- e.g. "cli", "http", "telegram"
  producer        TEXT NOT NULL,          -- e.g. "ironclaw"
  occurred_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),

  -- Payload (keep small; reference IDs; optionally include redacted text)
  payload         JSONB NOT NULL DEFAULT '{}'::jsonb,

  -- Operational
  inserted_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Idempotency: allow producer retries without duplicating events
CREATE UNIQUE INDEX IF NOT EXISTS ux_memory_outbox_dedupe
  ON memory_outbox (producer, dedupe_key);

-- Consumer poll path: fetch newest events for a workspace quickly
CREATE INDEX IF NOT EXISTS ix_memory_outbox_workspace_outbox_id
  ON memory_outbox (workspace_id, outbox_id);

CREATE INDEX IF NOT EXISTS ix_memory_outbox_type_time
  ON memory_outbox (event_type, occurred_at DESC);

CREATE INDEX IF NOT EXISTS ix_memory_outbox_conversation
  ON memory_outbox (conversation_id, outbox_id)
  WHERE conversation_id IS NOT NULL;

-- Per-consumer offsets (sequential consumption)
CREATE TABLE IF NOT EXISTS memory_outbox_offsets (
  consumer        TEXT NOT NULL,
  workspace_id    TEXT NOT NULL,
  last_outbox_id  BIGINT NOT NULL DEFAULT 0,
  updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (consumer, workspace_id)
);

-- Optional: processed event ids for non-sequential/idempotent consumers
CREATE TABLE IF NOT EXISTS memory_outbox_seen (
  consumer     TEXT NOT NULL,
  event_id     UUID NOT NULL,
  seen_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (consumer, event_id)
);

-- Retention helper index
CREATE INDEX IF NOT EXISTS ix_memory_outbox_inserted_at
  ON memory_outbox (inserted_at);
```

The schema intentionally supports:

- **Sequential consumption** (offset table) for high-throughput projection.
- **Idempotent “seen” tracking** for job-style consumers (useful if you ever shard processing or reprocess selectively).
- **Retention** keyed on `inserted_at` plus deletion policies in memoryd.

### Event types

Event types should be stable string constants. Keep payloads small; store IDs and minimal redacted text, not entire raw transcripts (raw text already exists in `conversation_messages.content`). citeturn14view3

| Event type | Producer boundary | Payload fields (minimum) | Idempotency key |
|---|---|---|---|
| `conversation.message.appended` | INSERT into `conversation_messages` citeturn14view3 | `message_id`, `role`, `content_hash`, `content_redacted?`, `created_at` | `conversation_message:{message_id}` |
| `workspace.document.upserted` | INSERT/UPDATE `memory_documents` citeturn15view0turn15view1 | `document_id`, `path`, `content_hash`, `updated_at` | `memory_document:{document_id}:{updated_at}` |
| `workspace.document.deleted` | DELETE `memory_documents` citeturn6view0 | `document_id`, `path`, `deleted_at` | `memory_document_deleted:{document_id}` |
| `memory.reinforcement.recorded` | IronClaw tool → memoryd Reinforce | `target_type`, `target_id`, `delta`, `source` | `reinforce:{job_id}:{target_type}:{target_id}` |
| `workspace.purged` | CLI/admin | `reason`, `requested_by` | `purge:{workspace_id}:{occurred_at_bucket}` |

Retention policy:

- Keep at least **7–30 days** of outbox events by default (align with memory hygiene defaults: conversations retention 7 days, daily logs 30 days). citeturn34view0turn35view2  
- Allow memoryd to delete from `memory_outbox` when:
  - `inserted_at < now() - retention_interval`, and
  - all active consumers’ `last_outbox_id` exceed the candidate rows.

### Example event JSONs

```json
{
  "event_id": "018f3af2-9c6a-7f4c-9a79-2a4b49c9dd0a",
  "dedupe_key": "conversation_message:7b8f8db5-8ac8-4f55-9bee-b9eaacd71c7b",
  "event_type": "conversation.message.appended",
  "workspace_id": "default",
  "conversation_id": "1c3f0fd9-fc5c-4f9b-8d02-4a2a1d1f18ac",
  "job_id": "3ab4dc9d-8c7a-4c7b-9b44-1b7f8d8b0fd2",
  "producer": "ironclaw",
  "occurred_at": "2026-03-11T09:12:44.103Z",
  "payload": {
    "message_id": "7b8f8db5-8ac8-4f55-9bee-b9eaacd71c7b",
    "role": "user",
    "content_hash": "sha256:4b158c...b0f",
    "content_redacted": "Let’s meet on [DATE] at [TIME] near [LOCATION]."
  }
}
```

```json
{
  "event_id": "018f3af2-a0cb-7a1b-8a77-9f4b3a9bfc1c",
  "dedupe_key": "memory_document:6a5ad356-9d12-4b36-8c0f-b3f4c0c77f21:2026-03-11T09:20:10Z",
  "event_type": "workspace.document.upserted",
  "workspace_id": "default",
  "producer": "ironclaw",
  "occurred_at": "2026-03-11T09:20:10.000Z",
  "payload": {
    "document_id": "6a5ad356-9d12-4b36-8c0f-b3f4c0c77f21",
    "path": "MEMORY.md",
    "content_hash": "sha256:9e8d1a...77a",
    "updated_at": "2026-03-11T09:20:10.000Z"
  }
}
```

## memoryd architecture and RPC surface

### High-level architecture

memoryd is a **local sidecar binary** (Rust) plus a small internal crate/module for shared types:

- `memoryd-core`: data model, capability verification, schemas, Qdrant/Oxigraph adapters.
- `memoryd`: UDS server, outbox consumer, Apalis worker runtime, metrics.
- `ironclaw` changes: UDS client + capability minting + outbox insertion hooks.

Mermaid architecture diagram:

```mermaid
flowchart LR
  subgraph IC[IronClaw process]
    Tools[Built-in tools\n(memory_search/write/read)]
    DB[(PostgreSQL)]
    Outbox[(memory_outbox)]
    Cap[Capability minting]
    UDSC[UDS client]
    Tools --> UDSC
    Cap --> UDSC
    Tools --> DB
    DB --> Outbox
  end

  subgraph MD[memoryd sidecar]
    UDSS[UDS server\nRPC methods]
    OC[Outbox consumer]
    WQ[Apalis workers\n(consolidation/reconcile)]
    KG[Oxigraph store\n(named graphs)]
    VDB[Qdrant\n(episodes, concepts)]
    LLM[Ollama\n(structured extraction + embeddings)]
    UDSS --> WQ
    OC --> WQ
    WQ --> KG
    WQ --> VDB
    WQ --> LLM
  end

  UDSC --> UDSS
  Outbox --> OC
```

This structure matches IronClaw’s existing “in-proc orchestrator” philosophy: sensitive operations (capabilities, secrets, durable storage) happen at the host boundary, not inside sandboxed tools. citeturn4view0turn17view2turn23view0

### RPC surface over UDS

To minimise attack surface, use a **single UDS endpoint** and expose only a small set of typed methods:

- `IngestEpisode`
- `Recall`
- `ReadFacts`
- `Reinforce`
- `ScheduleConsolidation`
- `Retract`
- `PurgeWorkspace`
- `Health`

The contract can be expressed as Protobuf messages (for strong typing) while still allowing JSON examples and an optional JSON-schema mirror for debugging.

#### Protobuf schema (core)

```proto
syntax = "proto3";
package memoryd.v1;

message Envelope {
  string request_id = 1;
  string capability_token = 2;
  oneof msg {
    IngestEpisodeRequest ingest_episode = 10;
    RecallRequest recall = 11;
    ReadFactsRequest read_facts = 12;
    ReinforceRequest reinforce = 13;
    ScheduleConsolidationRequest schedule_consolidation = 14;
    RetractRequest retract = 15;
    PurgeWorkspaceRequest purge_workspace = 16;
    HealthRequest health = 17;
  }
}

message EnvelopeResponse {
  string request_id = 1;
  oneof msg {
    IngestEpisodeResponse ingest_episode = 10;
    RecallResponse recall = 11;
    ReadFactsResponse read_facts = 12;
    ReinforceResponse reinforce = 13;
    ScheduleConsolidationResponse schedule_consolidation = 14;
    RetractResponse retract = 15;
    PurgeWorkspaceResponse purge_workspace = 16;
    HealthResponse health = 17;
    ErrorResponse error = 99;
  }
}

message ErrorResponse {
  string code = 1;        // e.g. "unauthorized", "invalid_argument", "internal"
  string message = 2;
  bool retryable = 3;
}

message EvidenceSpan {
  string source = 1;      // "conversation", "workspace.document"
  string source_id = 2;   // message_id or document_id
  uint32 start = 3;
  uint32 end = 4;
}

message IngestEpisodeRequest {
  string workspace_id = 1;
  string conversation_id = 2;
  string episode_id = 3;        // client-generated UUID for idempotency
  string text = 4;              // already redacted
  repeated EvidenceSpan evidence = 5;
  int64 occurred_at_unix_ms = 6;
  map<string,string> tags = 7;  // channel, etc.
}

message IngestEpisodeResponse {
  string episode_id = 1;
  bool accepted = 2;            // accepted for processing
}

message RecallRequest {
  string workspace_id = 1;
  string query = 2;
  uint32 limit = 3;
  repeated string hint_concept_ids = 4;
  map<string,string> constraints = 5; // optional: time window, etc.
}

message RecallHit {
  string kind = 1;              // "episode" | "fact" | "concept"
  string id = 2;
  double score = 3;
  string summary = 4;
  repeated EvidenceSpan evidence = 5;
}

message RecallResponse {
  repeated RecallHit hits = 1;
}

message ReadFactsRequest {
  string workspace_id = 1;
  repeated string fact_ids = 2;     // optional filter
  uint32 limit = 3;
}

message Fact {
  string fact_id = 1;
  string subject = 2;
  string predicate = 3;
  string object = 4;
  double confidence = 5;
  repeated EvidenceSpan evidence = 6;
  int64 first_seen_unix_ms = 7;
  int64 last_seen_unix_ms = 8;
  bool retracted = 9;
}

message ReadFactsResponse {
  repeated Fact facts = 1;
}

message ReinforceRequest {
  string workspace_id = 1;
  string target_kind = 2;     // "episode"|"fact"|"concept"
  string target_id = 3;
  double delta = 4;           // positive reinforcement
  string reason = 5;
}

message ReinforceResponse { bool ok = 1; }

message ScheduleConsolidationRequest {
  string workspace_id = 1;
  string reason = 2;          // "idle", "manual", "threshold"
}

message ScheduleConsolidationResponse { string job_id = 1; }

message RetractRequest {
  string workspace_id = 1;
  string target_kind = 2;
  string target_id = 3;
  string reason = 4;
}

message RetractResponse { bool ok = 1; }

message PurgeWorkspaceRequest {
  string workspace_id = 1;
  string confirmation = 2;  // e.g. "PURGE"
}

message PurgeWorkspaceResponse { bool ok = 1; }

message HealthRequest {}
message HealthResponse {
  string status = 1;  // "ok"
  string version = 2;
}
```

This keeps the “RPC surface” small, auditable, and easy to fuzz.

#### UDS message examples

```json
{
  "request_id": "req_01",
  "capability_token": "eyJhbGciOiJFZERTQSIsImtpZCI6Im1lbW9yeWQtc2VzczEifQ.eyJpc3MiOiJpcm9uY2xhdyIsImF1ZCI6Im1lbW9yeWQiLCJzdWIiOiJkZWZhdWx0Iiwid3MiOiJkZWZhdWx0Iiwiam9iIjoiM2FiNGRjOWQtOGM3YS00YzdiLTliNDQtMWI3ZjhkOGIwZmQyIiwic2NwIjpbIm1lbW9yeS5pbmdlc3QiXSwiaWF0IjoxNzQxNjgyMDAwLCJleHAiOjE3NDE2ODIwNjB9.<sig>",
  "ingest_episode": {
    "workspace_id": "default",
    "conversation_id": "1c3f0fd9-fc5c-4f9b-8d02-4a2a1d1f18ac",
    "episode_id": "018f3b0a-0a9f-7b2d-8f1c-21a8dd3b31b2",
    "text": "We agreed to meet on [DATE] at [TIME].",
    "occurred_at_unix_ms": 1773229964103,
    "tags": {"channel": "cli"}
  }
}
```

### Capability token format and enforcement model

IronClaw already uses job-scoped bearer authentication for internal worker calls. citeturn7view0 For memoryd, use a **JWT-like signed token** carrying explicit scopes and resource bindings.

#### Token structure (JWT-like)

- `header`: `{"alg":"EdDSA","kid":"memoryd-session-1","typ":"cap+jwt"}`
- `payload` (claims):
  - `iss`: issuer, e.g. `ironclaw`
  - `aud`: audience, e.g. `memoryd`
  - `sub`: workspace principal (`default` in Axinite’s current single-user pattern) citeturn23view0
  - `ws`: workspace_id (must match request)
  - `job`: job_id (optional, but recommended for auditing)
  - `scp`: array of scopes (see table below)
  - `iat`, `exp`: issued-at and expiry
  - `jti`: unique token id (optional)
  - `cnf`: confirmation binding (optional; e.g. hash of peer uid + socket path)

#### Scope model

| RPC method | Required scope(s) | Notes |
|---|---|---|
| `Health` | `memory.health` | No workspace binding required, but still require local peer checks |
| `IngestEpisode` | `memory.ingest` | Must bind to `ws`; idempotent by `episode_id` |
| `Recall` | `memory.recall` | Read-only, but can leak; keep separate from ingest |
| `ReadFacts` | `memory.readfacts` | More sensitive than recall if facts carry normalized PII |
| `Reinforce` | `memory.reinforce` | Writes “dopamine-like” reinforcement values |
| `ScheduleConsolidation` | `memory.consolidate` | Triggers background jobs |
| `Retract` | `memory.retract` | Marks facts/episodes as retracted |
| `PurgeWorkspace` | `memory.purge` | Highest privilege; short TTL + explicit confirmation |

#### Enforcement steps inside memoryd

For each request:

1. **UDS peer verification**: ensure the connecting peer UID matches the memoryd UID; reject otherwise. (Linux can enforce via peer-credentials; on macOS use equivalent where available.)  
2. **Token verification**: check signature (public key), `aud`, `iss`, `exp`, and `ws` claim equality with request workspace.
3. **Scope check**: map method → required scope(s).
4. **Request validation**: reject oversized payloads; validate IDs; enforce “no network exposure” policy (memoryd must not provide any network API itself).
5. **Audit log**: record `job_id`, method, workspace, target ids, and decision outcome.

Redaction and leak detection: before IronClaw sends an episode text to memoryd, run the existing leak detector and either block or redact secret-like content. citeturn36view0turn36view1 This avoids “remembering secrets” as retrievable embeddings.

## Storage schemas in Qdrant and Oxigraph

### Qdrant: episodes and concepts collections

Qdrant’s data model uses “collections” containing “points” (id + vectors + payload). Collections can use **named vectors** to attach multiple vector spaces with distinct dimensions and distance metrics. citeturn33search1turn33search9 Payload indexing improves filtered search performance. citeturn33search6

#### Collection-per-workspace mapping (recommended)

Create two collections per workspace:

- `icw_{workspace_id}_episodes`
- `icw_{workspace_id}_concepts`

Rationale: simplifies purge, reduces risk of cross-tenant filter mistakes, and makes deletion propagation crisp (drop collections). This is consistent with the “capabilities must be explicit” philosophy in IronClaw’s WASM model. citeturn17view0

#### Episodes collection schema

Vectors (named):

- `dense`: the embedding vector for episode summaries / chunks
  - `size`: derived from the configured embedding model. Axinite already supports dimensions per model (e.g. `nomic-embed-text`→768, `mxbai-embed-large`→1024, OpenAI 1536/3072). citeturn20view3turn16view0  
  - `distance`: `Cosine` (typical for text embeddings). citeturn33search1turn33search0

Optional future vectors:

- `sparse`: if you add a sparse retriever; enables Qdrant hybrid queries with RRF or formula fusion. citeturn26search2turn33search4  

Payload fields:

- `episode_id` (UUID string) — same as point id (dup ok)
- `workspace_id` (string)
- `conversation_id` (UUID string)
- `occurred_at` (datetime)
- `summary` (string, redacted; short)
- `content_redacted` (string, optional; may omit to minimise sensitive storage)
- `content_hash` (string)
- `evidence` (array of `{source,source_id,start,end}`)
- reinforcement signals:
  - `strength` (float, default 0)
  - `last_reinforced_at` (datetime)
  - `retrieved_count` (int)
- lifecycle:
  - `retracted` (bool)
  - `purge_at` (datetime optional)

Indexes:

- payload index on `occurred_at` (datetime) for time-based reranking and filtering. citeturn33search6turn31search1  
- payload index on `retracted` (bool)
- payload index on `conversation_id` (keyword)

#### Concepts collection schema

Purpose: store “concept nodes” (entities, topics, projects), each linked to evidence and facts.

Vectors:

- `dense`: embedding of concept label + canonical description.

Payload:

- `concept_id` (UUID)
- `label` (string)
- `type` (string: person/org/place/project/etc.)
- `aliases` (string[])
- `salience` (float)
- `first_seen_at`, `last_seen_at` (datetime)
- `evidence` spans and provenance
- `retracted` (bool)

This supports “associative” recall: concept search → related episodes and facts.

### Qdrant reranking and “dopamine-like” reinforcement

Qdrant’s query API supports hybrid retrieval and fusion, including an RRF query option. citeturn26search2turn4view1 It also supports formula-based reranking with decay functions (e.g. exponential decay on timestamps) and requires payload variables used in formulas to be indexed. citeturn31search1turn33search6

A practical reranking formula for episodes could combine:

- vector similarity score (`$score`)
- recency decay on `occurred_at`
- reinforcement boost (`strength`)

Example (conceptual JSON; keep in memoryd as a builder):

```json
{
  "prefetch": {
    "query": [0.01, 0.45, 0.67],
    "using": "dense",
    "limit": 50
  },
  "query": {
    "formula": {
      "sum": [
        "$score",
        { "mult": [0.15, { "exp_decay": {
          "x": { "datetime_key": "occurred_at" },
          "scale": 604800
        }}]},
        { "mult": [0.10, { "key": "strength" }]}
      ]
    }
  },
  "limit": 10
}
```

This approximates a “dopamine reward” mechanism: reinforcement raises future retrieval probability, while recency decays naturally.

Security caution: Qdrant’s own security guide notes that internal communication channels (e.g. internal gRPC in distributed mode) are not protected by API keys; you must keep those ports private. citeturn27search0 For a local single-node setup, run Qdrant bound to loopback with API keys, and ensure only memoryd can reach it (local firewall / OS sandboxing).

### Oxigraph named-graph schema, predicates, provenance, SPARQL patterns

Oxigraph is a Rust graph database implementing SPARQL; its Store API enforces that only one read-write store can be open simultaneously for a given path (read-only stores can be opened separately). citeturn28search0turn28search1 This makes it well-suited as an embedded, non-networked store inside memoryd, with a single writer (Apalis worker) and optional read-only handles for query execution.

#### Named graph layout

Use **named graphs per workspace** to strongly separate data:

- Graph: `urn:ironclaw:ws:{workspace_id}:facts`
- Graph: `urn:ironclaw:ws:{workspace_id}:provenance`
- Graph: `urn:ironclaw:ws:{workspace_id}:retractions`

#### RDF predicates (suggested vocabulary)

Use a minimal internal vocabulary (do not depend on remote ontologies):

- `ic:Fact` (class)
- `ic:Concept` (class)
- `ic:predicate` (property – if you model facts as reified nodes)
- `ic:subject`, `ic:object`
- `ic:confidence` (xsd:double)
- `ic:firstSeen`, `ic:lastSeen` (xsd:dateTime)
- `ic:derivedFrom` (provenance link to evidence span)
- `ic:evidenceSpan` (stringified JSON or structured nodes)
- `ic:retracted` (xsd:boolean)
- `ic:retractionReason` (string)
- `ic:hash` (xsd:string for content hashing)

#### Provenance model

Each extracted fact links to one or more evidence spans:

- Evidence span node: `urn:ironclaw:evidence:{source_id}:{start}:{end}:{hash}`
- Properties:
  - `ic:source` = `"conversation"` or `"workspace.document"`
  - `ic:sourceId` = message_id or document_id
  - `ic:start`, `ic:end`
  - `ic:contentHash`

This allows:
- Deletion propagation: when a conversation message is deleted/expired, memoryd can find facts derived solely from those spans and retract or drop them.
- Auditable “why does the assistant believe this?” queries.

#### SPARQL access patterns (no network exposure)

memoryd uses embedded SPARQL queries:

- Retrieve top facts by concept:
  - `SELECT ?fact ?pred ?obj ?conf WHERE { ... } ORDER BY DESC(?conf) LIMIT N`
- Retrieve facts with evidence:
  - `CONSTRUCT { ... } WHERE { ... }`
- Mark retraction:
  - `DELETE/INSERT` within updates, but keep retractions in a separate named graph for auditability.

Avoid spinning up Oxigraph’s HTTP server; keep access strictly in-process.

## Ollama extraction contract and Apalis consolidation pipeline

### Ollama extraction with structured outputs

Ollama supports **structured outputs** by providing a JSON schema to a `format` field (and recommends also embedding the schema in the prompt). citeturn29search1turn29search4 It also provides an embeddings endpoint where vector dimension depends on the embedding model. citeturn29search2turn29search5 For strict locality, disable cloud features via config or `OLLAMA_NO_CLOUD=1`. citeturn29search0

#### Extraction contract (JSON schema)

This contract standardises what memoryd expects from the extraction model and keeps the rest of the pipeline deterministic.

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "MemoryExtraction",
  "type": "object",
  "required": ["summary", "entities", "relations", "candidate_facts", "confidence", "evidence_spans"],
  "properties": {
    "summary": {"type": "string", "maxLength": 1200},
    "confidence": {"type": "number", "minimum": 0, "maximum": 1},
    "entities": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["entity_id", "label", "type", "confidence"],
        "properties": {
          "entity_id": {"type": "string"},
          "label": {"type": "string"},
          "type": {"type": "string"},
          "aliases": {"type": "array", "items": {"type": "string"}},
          "confidence": {"type": "number", "minimum": 0, "maximum": 1}
        }
      }
    },
    "relations": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["subject_entity_id", "predicate", "object_entity_id", "confidence"],
        "properties": {
          "subject_entity_id": {"type": "string"},
          "predicate": {"type": "string"},
          "object_entity_id": {"type": "string"},
          "confidence": {"type": "number", "minimum": 0, "maximum": 1}
        }
      }
    },
    "candidate_facts": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["subject", "predicate", "object", "confidence"],
        "properties": {
          "subject": {"type": "string"},
          "predicate": {"type": "string"},
          "object": {"type": "string"},
          "confidence": {"type": "number", "minimum": 0, "maximum": 1},
          "fact_type": {"type": "string"},
          "proposed_ttl_days": {"type": "integer", "minimum": 1, "maximum": 3650}
        }
      }
    },
    "evidence_spans": {
      "type": "array",
      "items": {
        "type": "object",
        "required": ["source", "source_id", "start", "end"],
        "properties": {
          "source": {"type": "string"},
          "source_id": {"type": "string"},
          "start": {"type": "integer", "minimum": 0},
          "end": {"type": "integer", "minimum": 0}
        }
      }
    }
  }
}
```

Ingest flow should:

- Redact secrets before sending text to Ollama using IronClaw’s leak detector rules. citeturn36view1turn36view0  
- Keep embeddings consistent: use the same embedding model for:
  - Episode vectors at ingestion,
  - Query vectors at recall.
  Axinite already records and configures an embedding provider + dimension. citeturn20view4turn20view5  

### Apalis workers: jobs, retries, heuristics, reconciliation

Apalis provides middleware-like features (retry, timeout, concurrency limiting) and apalis-postgres provides a reliable Postgres-backed backend with heartbeats and orphan re-enqueueing. citeturn30search0turn30search1turn30search19

#### Job types (embedded in memoryd)

| Job | Trigger | Core work | Idempotency key |
|---|---|---|---|
| `ProjectOutboxBatch` | periodic poll / notify | consume outbox rows → enqueue finer jobs | `(consumer, last_outbox_id)` |
| `ExtractEpisode` | `conversation.message.appended` aggregation | call Ollama structured extraction; embed; upsert Qdrant episode | `episode_id` |
| `UpsertConcepts` | output of extraction | merge entities into concept store; upsert Qdrant concepts | `hash(label,type,ws)` |
| `UpsertFacts` | output of extraction | write RDF triples into Oxigraph named graphs | `hash(fact,ws,evidence)` |
| `ConsolidateWorkspace` | scheduled / idle | cluster similar facts; promote stable facts; decay stale ones | `ws:consolidate:timebucket` |
| `ReconcileDeletes` | hygiene or explicit deletion | retract facts whose evidence expired; purge Qdrant points | `ws:reconcile:timebucket` |

#### Consolidation heuristics and promotion rules

A minimal, implementable rule-set:

- **Promotion threshold**: promote candidate facts to “stable” if:
  - seen in ≥2 distinct episodes OR
  - reinforced by user feedback (`Reinforce`) OR
  - persisted in `MEMORY.md` (curated).
- **Conflict handling**: if two facts conflict (same subject+predicate, different object), keep both as candidates but lower confidence unless one dominates by:
  - recency-weighted support,
  - reinforcement,
  - explicit curated source.
- **Decay**: reduce `strength` by exponential decay over time; Qdrant reranking can incorporate `exp_decay` on timestamps. citeturn31search1turn27search0  
- **Retraction propagation**: `Retract` marks `retracted=true` in Qdrant payload and inserts a triple in Oxigraph retractions graph (do not hard-delete immediately unless `PurgeWorkspace`).

Retries:

- Use Apalis retry middleware for transient failures (Ollama not ready; Qdrant connection). citeturn30search0  
- Treat extraction as retryable; treat graph corruption as non-retryable (surface as Health degraded).

## Security considerations, rollout plan, tests, monitoring

### Security posture

**Minimise exposure**

- memoryd listens only on a UDS socket (0600 permissions; owned by the user running IronClaw).
- Do not expose Oxigraph over HTTP; embed it in memoryd only. citeturn28search1  
- Ensure Ollama and Qdrant bind to loopback; disable Ollama cloud features for strict locality. citeturn29search0turn29search7  
- Configure Qdrant API keys; keep internal ports inaccessible (especially if distributed mode ever enabled). citeturn27search0  

**Least privilege / capability grants**

- Mint a short-lived token per request with minimal scope.
- Split read scopes (`recall`, `readfacts`) from write scopes (`ingest`, `reinforce`, `retract`, `purge`).
- Require explicit operator confirmation for `PurgeWorkspace`.

**Audit logging**

Store in Postgres (append-only) or in IronClaw’s existing history/audit tables:

- request: `job_id`, method, workspace, target ids
- decision: allow/deny, reason
- latency and error codes

Axinite already persists job history and tool actions (tool_name, inputs/outputs). citeturn11view0 Use the same “structured event” conventions.

**Redaction**

- Reuse the existing leak detector logic to redact/block secret-like content before ingestion. citeturn36view1turn36view0  
- Never store raw secrets in Qdrant payloads or Oxigraph literals.  
- Prefer storing only hashes + short summaries where possible.

**Deletion propagation**

- Existing workspace hygiene deletes old `daily/` and `conversations/` docs based on retention settings. citeturn35view2turn34view0  
- When those deletions occur, emit outbox events so memoryd can:
  - retract corresponding facts (provenance-based),
  - delete Qdrant points, and
  - compact Oxigraph graphs (optional).

### Rollout plan

**Disabled → Shadow → Active**

- **Disabled (default)**: ship schema and code behind flags; no behavioural change.
- **Shadow**:
  - Start memoryd, write outbox events, run extraction, populate Qdrant/Oxigraph.
  - Keep user-facing retrieval using the current workspace hybrid search only. citeturn4view1turn6view0  
  - Record metrics: recall overlap vs baseline, latency, errors.
- **Active**:
  - `memory_search` first calls memoryd `Recall`, then optionally falls back to existing workspace search if memoryd is unavailable or returns empty.
  - Keep a “kill switch” env var: `MEMORY_SIDECAR_MODE=disabled`.

### Test plan

**Relevance and correctness**

- Golden-set recall tests: verify memoryd returns expected episodes/facts for known conversations.
- Regression tests for hybrid ranking (ensure reinforcement + recency boosts behave monotonically).

**Security tests**

- ACL bypass attempts:
  - invalid signature
  - wrong `ws`
  - missing scope
  - expired token
- UDS permission tests: ensure socket path created as 0600 and rejects foreign UID.

**Data safety**

- Redaction tests: feed known token patterns into ingestion; assert leak detector redacts/blocks. citeturn36view1turn36view0  
- Purge tests: invoke `PurgeWorkspace`; assert Qdrant collections gone, Oxigraph graphs deleted, and outbox offsets reset.

**Benchmarks**

- p50/p95 latency for:
  - outbox consumption batch
  - Recall (embedding + Qdrant query + optional graph lookup)
  - consolidation job runtime

### Monitoring and metrics

Use IronClaw’s observability hooks (currently supports noop/log backends) and add structured tracing spans around:

- outbox lag (max(outbox_id) - offset)
- extraction success/failure counts
- Qdrant query latency
- Apalis queue depth, retries, orphan re-enqueues citeturn30search1turn30search19  

If you later add Prometheus, Apalis already has a `prometheus` feature flag. citeturn30search0  

---

**Primary sources used**: Axinite/IronClaw repository docs and code for architecture, workspace, tooling, migrations, and safety; Qdrant official docs for collections/named vectors/hybrid queries/security and payload indexing; Oxigraph docs for embedded store constraints; Ollama docs for structured output and embeddings APIs and for disabling cloud features; Apalis docs for worker/job-queue features and Postgres backend behaviour. citeturn4view0turn4view1turn15view2turn17view0turn36view0turn33search1turn26search2turn27search0turn28search0turn29search1turn29search5turn30search1
