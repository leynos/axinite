# RFC 0013: Memory projection tiers and promotion rules

## Preamble

- **RFC number:** 0013
- **Status:** Proposed
- **Created:** 2026-03-15

## Summary

This RFC specifies memory semantics for the sidecar architecture
proposed in RFC 0007: projection classes, epistemic status, observer
and subject scope, promotion thresholds, contradiction handling, and
reconciliation metadata. RFC 0007 establishes the process boundary,
locality, transactional outbox, Qdrant/Oxigraph split,
reinforcement, and retraction model. [^1] This RFC fills the gap
between "memory plumbing" and "trustworthy recall".

The Honcho analysis's main warning is that without explicit projection
lifecycle rules, memory becomes extraction plus retrieval rather than
trustworthy recall. [^2] A memory system that silently flattens "the
user said X" and "the model inferred Y" into the same kind of truth
atom produces epistemology by blender.

## Problem

RFC 0007 proposes a sidecar (`memoryd`) that consumes a transactional
outbox, extracts entities and facts with structured outputs, stores
vectors in Qdrant and normalized facts in Oxigraph, and delegates
consolidation to Apalis workers. [^1] The sidecar design handles
transport, storage, and process isolation well, but it does not yet
specify:

- **What kinds of memory artefacts exist.** RFC 0007 mentions episodes,
  concepts, and facts, but does not define a closed taxonomy or the
  relationships between types.
- **How confident the system is in each artefact.** There is no
  distinction between "the user explicitly stated X", "the model
  deduced Y from stated facts", and "the model hypothesized Z from
  weak signals".
- **Who observed what about whom.** Memory artefacts lack
  observer/subject metadata, which becomes critical for multi-agent
  scenarios, privacy-sensitive use cases, and shared workspaces.
- **How artefacts mature.** There are no promotion rules governing
  when a hypothesis becomes a trusted fact, when a transient
  observation becomes a stable profile trait, or when a summary
  replaces a set of episodes.
- **How contradictions are resolved.** When new evidence contradicts
  an existing fact, the system needs a defined resolution strategy.

The Honcho system demonstrates that a usable memory system needs
layered representations, promotion rules, scoped perspective, and a
reconciler that assumes the world is messy. [^2]

## Current state

### RFC 0007 data model

RFC 0007 proposes:

- **Transactional outbox**: events published by IronClaw at persistence
  boundaries (conversation messages, workspace document mutations).
- **Extraction**: structured extraction via Ollama, producing
  entities, facts, and relations.
- **Storage**: vectors in Qdrant (collection-per-workspace), normalized
  facts in Oxigraph (embedded, no network exposure).
- **Recall**: hybrid retrieval combining vector similarity and graph
  queries.
- **Consolidation**: Apalis workers for background reconciliation,
  retries, and heavyweight extraction.

### Gaps relative to Honcho patterns

| Honcho pattern | RFC 0007 status | Gap |
| --- | --- | --- |
| Explicit observations | Extraction produces entities/facts | No epistemic status labelling. |
| Deductive conclusions | Not distinguished from observations. | All extraction outputs are treated equally. |
| Summaries | Compaction writes summaries to workspace. | Summaries are not first-class memory projections. |
| Stable profile/card | Not present. | Durable user preferences mixed with transient observations. |
| Observer/observed scoping | Not present. | Memory is workspace-scoped, not perspective-scoped. |
| Ordered derivation | Outbox provides ordering. | No batching/windowing before extraction. |
| Idle-time consolidation | Apalis workers handle consolidation. | No explicit idle/threshold/contradiction triggers. |
| Reconciliation metadata | Retraction and deletion propagation proposed. | No sync state or replay semantics per target. |

_Table 1: RFC 0007 gaps relative to Honcho patterns._

## Goals and non-goals

- Goals:
  - Define a closed set of projection classes with clear semantics.
  - Add epistemic status to every memory artefact.
  - Add observer and subject scope to every artefact.
  - Define promotion rules governing how artefacts mature.
  - Define contradiction-handling strategies.
  - Add reconciliation metadata and sync state.
  - Specify recall behaviour across projection layers.
- Non-goals:
  - Redesign the sidecar architecture. This RFC extends RFC 0007, not
    replaces it.
  - Specify embedding models, extraction prompts, or retrieval
    algorithms. Those remain configurable.
  - Import Honcho's peer-centric ontology. Axinite's memory model
    supports person, project, task, document, and event concepts on
    equal footing, not only peer representations.

## Proposed design

### 1. Projection classes

Define five first-class projection classes:

| Class | Description | Storage | Lifetime |
| --- | --- | --- | --- |
| `episode` | A bounded, chronological record of an interaction or event. Contains raw observations, tool call results, and contextual signals. | Qdrant (vector) + Oxigraph (relations) | Indefinite; subject to retention policy. |
| `summary` | A distilled representation of one or more episodes. Cheaper to retrieve than replaying raw episodes. | Qdrant (vector) | Replaced when source episodes change. |
| `concept` | A named entity, topic, or category extracted from episodes. Connects related episodes and facts. | Oxigraph (node) + Qdrant (vector) | Indefinite. |
| `fact` | A discrete, verifiable claim about the world or a subject. Has epistemic status and confidence. | Oxigraph (triple/quad) | Until retracted or superseded. |
| `profile` | A stable, curated representation of an entity (person, project, system). Aggregates durable traits, preferences, and constraints. | Oxigraph (subgraph) + Qdrant (vector) | Long-lived; updated only through explicit promotion. |

_Table 2: Projection classes._

### 2. Epistemic status

Every `fact` and `profile` entry carries an epistemic status:

| Status | Meaning | Trust level | Promotion path |
| --- | --- | --- | --- |
| `explicit` | Directly stated by a human (user or operator). | Highest | N/A (already trusted). |
| `curated` | Reviewed and confirmed by an operator. | High | Promoted from any status by operator action. |
| `deduced` | Logically derived from explicit or curated facts. | Medium | Automatic if premises are trusted. |
| `hypothesized` | Inductively or abductively inferred. | Low | Requires corroboration or curation. |
| `retracted` | Previously held but now contradicted or withdrawn. | None | Retracted facts remain for audit trail. |

_Table 3: Epistemic status levels._

Episodes and summaries do not carry epistemic status. They are
evidence, not claims. Concepts inherit the status of the strongest
supporting evidence.

### 3. Observer and subject scope

Every memory artefact carries:

- **`observer_id`**: the entity that produced or observed the artefact
  (e.g. a specific agent instance, user, or routine).
- **`subject_id`**: the entity that the artefact is about (e.g. a user,
  project, or external system).
- **`scope`**: visibility constraint (`private`, `workspace`, `shared`).
- **`audience`**: optional list of entities permitted to recall this
  artefact.

This addresses the Honcho analysis's observation that memory is not
workspace-global truth but a representation from some vantage point
about some subject. [^2] It also future-proofs the sidecar for
multi-agent and privacy-sensitive use cases.

### 4. Promotion rules

Promotion governs how artefacts move between epistemic levels:

**Hypothesis to deduced**: a hypothesized fact is promoted to deduced
when the supporting evidence chain reaches a configurable confidence
threshold (e.g. corroborated by N independent episodes or supported by
M explicit facts).

**Deduced to curated**: a deduced fact is promoted to curated only by
explicit operator action (approval via UI, workspace edit, or tool
call).

**Anything to explicit**: an artefact is marked explicit when a human
directly states or confirms it in conversation or workspace.

**Anything to profile**: a fact or observation is promoted to the
profile layer when it meets all of:

- epistemic status is `explicit` or `curated`,
- the fact has been stable (unrebutted) for a configurable duration,
- the fact describes a durable trait rather than a transient state.

**Demotion**: curated or deduced facts that are contradicted by new
explicit evidence are demoted to `retracted` with a link to the
contradicting evidence.

### 5. Contradiction handling

When new evidence contradicts an existing fact:

1. If the new evidence is `explicit` and the existing fact is
   `hypothesized` or `deduced`, retract the existing fact
   automatically.
2. If both the new evidence and the existing fact are `explicit` or
   `curated`, flag the contradiction for operator resolution. Neither
   is retracted automatically.
3. If the new evidence is `hypothesized` and the existing fact is
   `curated` or `explicit`, the new evidence is recorded but does not
   retract the existing fact.

Contradiction records are stored as Oxigraph relations linking the
conflicting facts, with timestamps and evidence references.

### 6. Reconciliation metadata

Every projection target (Qdrant collection, Oxigraph graph) carries
sync metadata:

| Field | Type | Description |
| --- | --- | --- |
| `projection_id` | UUID | Unique identifier for the projection. |
| `target` | enum | `qdrant`, `oxigraph`. |
| `status` | enum | `pending`, `synced`, `failed`. |
| `retry_count` | integer | Number of sync attempts. |
| `last_error` | string | Most recent error message. |
| `last_synced_at` | timestamp | Last successful sync time. |
| `deleted_soft` | boolean | Marked for deletion. |
| `deleted_hard` | boolean | Physically removed from target. |

_Table 4: Reconciliation metadata fields._

If Qdrant or Oxigraph is unavailable, `memoryd` marks projections as
`pending`, retries with backoff, and exposes health and lag metrics.
The Honcho analysis argues strongly for this: memory synchronization
to external stores can drift and must be repaired. [^2]

### 7. Recall across projection layers

Recall should query each projection layer separately before synthesis:

1. **Profile recall**: retrieve stable traits and preferences from the
   profile layer. These are treated as high-confidence context.
2. **Fact recall**: retrieve relevant facts from Oxigraph, filtered by
   epistemic status. Deduced and curated facts are included; hypotheses
   are included only if explicitly requested.
3. **Summary recall**: retrieve relevant summaries from Qdrant for
   context compression.
4. **Episode recall**: retrieve raw episodes only when detail beyond
   summaries and facts is needed.
5. **Synthesis**: combine results from all layers, annotating each
   with its projection class and epistemic status.

This layered recall avoids mixing raw episodic retrieval with curated
facts in a single vector search, which is the "one vector soup"
failure mode the Honcho analysis warns against. [^2]

### 8. Batching and windowing

Extraction should not trigger on every individual message. A batching
layer between outbox consumption and extraction should support:

- `min_context_tokens`: minimum accumulated tokens before triggering
  extraction.
- `max_batch_delay_ms`: maximum time before flushing a batch.
- `flush_on_idle`: trigger extraction when the conversation goes quiet.
- `flush_on_explicit_write`: trigger extraction when a workspace
  document is explicitly written.

Batching by `(workspace_id, conversation_id, entity_scope)` ensures
that extraction operates on coherent conversational units rather than
half-formed turns. [^2]

## Requirements

### Functional requirements

- All memory artefacts must carry a projection class, epistemic status,
  observer/subject scope, and provenance metadata.
- Promotion between epistemic levels must follow explicit rules, not
  implicit model behaviour.
- Contradictions must be detected, recorded, and resolved according to
  the defined strategy.
- Recall must query projection layers separately and annotate results
  with class and status.
- Reconciliation metadata must track sync state per projection per
  target.

### Technical requirements

- The projection class taxonomy must be extensible without schema
  migration for existing artefacts.
- Epistemic status must be stored as a first-class field in both
  Qdrant payloads and Oxigraph triples/quads.
- Observer and subject scope must be enforceable at the recall boundary
  (only artefacts within the caller's scope are returned).
- Batching parameters must be configurable per workspace.

## Compatibility and migration

This RFC extends RFC 0007. Existing memory artefacts (if any exist from
a prototype) should be migrated by assigning default values:

- Projection class: `episode` for conversation-derived artefacts,
  `fact` for extracted entities/relations.
- Epistemic status: `hypothesized` for model-extracted artefacts,
  `explicit` for user-stated artefacts.
- Observer: the agent instance that performed the extraction.
- Subject: inferred from the artefact content.
- Scope: `workspace`.

## Alternatives considered

### Option A: Single-tier memory with metadata tags

Store all memory artefacts in one pool and use metadata tags for
filtering rather than separate projection classes. Simpler to implement
but harder to enforce promotion rules and harder to query efficiently
across trust levels.

### Option B: Import Honcho's peer-centric ontology

Adopt Honcho's full observer/observed model built around "social
cognition". This is too narrow for Axinite's broader scope (projects,
tasks, documents, events on equal footing with persons). The
transferable lesson is perspective-aware scoping, not the peer-centric
frame.

## Open questions

- Are summaries and profiles first-class physical stores or logical
  views over facts and episodes? The proposed design treats them as
  physical stores for performance and simplicity, but a logical-view
  approach would reduce data duplication.
- Should recall query each projection layer separately before
  synthesis, or should a unified retrieval engine handle layer
  selection internally? The proposed design exposes layers to allow
  callers to control trust levels, but this adds API complexity.
- How do curated facts override hypotheses, and how do deletions or
  privacy requests propagate through promoted artefacts? The proposed
  retraction mechanism handles fact-level contradictions, but profile
  entries derived from retracted facts may need cascading updates.
- Should the batching/windowing layer converge conversation and
  workspace-document events into the same derivation pipeline? The
  Honcho analysis suggests yes: the sidecar should care about evidence
  streams, not their origin. [^2]

## Recommendation

Implement the five projection classes (episode, summary, concept, fact,
profile) with explicit epistemic status, observer/subject scope, and
promotion rules. Add reconciliation metadata with sync state and health
metrics. Restructure recall to query projection layers separately.
Add a batching layer between outbox consumption and extraction. These
changes extend RFC 0007's sidecar without requiring architectural
rework.

---

[^1]: RFC 0007: Secure memory sidecar design. See
    `docs/rfcs/0007-secure-memory-sidecar-design.md`.

[^2]: Honcho analysis, memory projection and promotion lessons. See
    `docs/ChatGPT-Analysis_of_Honcho_System.md`.
