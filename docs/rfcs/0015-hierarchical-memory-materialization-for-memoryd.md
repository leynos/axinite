# RFC 0015: Hierarchical memory materialization for memoryd

## Preamble

- **RFC number:** 0015
- **Status:** Proposed
- **Created:** 2026-03-18
- **Amends:** RFC 0007: Secure memory sidecar design for axinite
- **Related:** RFC 0014: Memory projection tiers and promotion rules; RFC 0016:
  Theme detection and sparsity rebalancing for `memoryd`; RFC 0017:
  Hierarchical recall for `memoryd`
- **Companion ADRs:** ADR 003: Theme management belongs in `memoryd`;
  ADR 004: Dual-path semantic extraction with validated provenance;
  ADR 005: Dual-mode uncertainty gating for hierarchical recall

## Summary

This RFC outlines how `memoryd` can materialize a durable hierarchical
structure over raw evidence without replacing RFC 0014's projection
taxonomy. RFC 0014 remains authoritative for projection classes,
epistemic status, promotion rules, contradiction handling, and recall
trust semantics. This RFC instead addresses the consolidation-layer
question: how raw messages and document revisions are grouped into
episodes, how reusable semantic carriers are derived from that
evidence, and how optional theme structures can support navigation and
retrieval without becoming a second truth model.

The proposed hierarchy is:

- raw evidence (`Message` or `DocumentSpan`)
- `Episode` materializations
- semantic carriers
- optional `Theme` groupings

Compatibility rule: `Episode`, semantic-carrier, and `Theme` are
storage and navigation structures. They do not replace RFC 0014's
first-class memory artefacts (`episode`, `summary`, `concept`, `fact`,
and `profile`). In particular, `Theme` is not a new epistemic class,
and semantic carriers do not bypass RFC 0014 promotion or provenance
rules.

## Problem

RFC 0014 defines what kinds of memory artefacts exist and how their
trust and maturity change over time, but it intentionally does not
specify the physical hierarchy or projection pipeline that produces
those artefacts. That leaves several implementation questions open:

- how raw evidence is grouped into durable episode units
- how extracted long-term statements are represented before and during
  RFC 0014 promotion
- how temporal and provenance edges are stored so recall can answer
  ordering and validity questions
- whether higher-level theme navigation is durable, query-time only, or
  absent

Without a companion materialization design, `memoryd` risks either a
flat vector store with weak lineage or an accidental second taxonomy
that conflicts with RFC 0014.

## Goals and non-goals

- Goals:
  - Define a consolidation-layer hierarchy that is explicitly
    compatible with RFC 0014.
  - Define source-of-truth boundaries across PostgreSQL, Oxigraph, and
    Qdrant.
  - Define outline contracts for episode materialization, semantic
    carriers, and theme structures.
  - Define the provenance and temporal guarantees that the hierarchy
    must preserve.
  - Define how curated documents and retractions flow through the
    hierarchy.

- Non-goals:
  - Redefine RFC 0014 projection classes or epistemic states.
  - Specify the full retrieval algorithm or ranking policy.
  - Specify exact embedding models or extraction prompts.
  - Move theme identity or hierarchy policy into `chutoro-core`.

## Compatibility with RFC 0014

RFC 0014 remains the normative document for memory semantics.

- **Episodes** map directly to RFC 0014's `episode` projection class.
- **Semantic carriers** are consolidation-layer structures that may
  materialize candidate or stable `fact`, `concept`, and profile-bound
  claims. They are not a sixth user-facing projection class.
- **Themes** are navigation and retrieval groupings over semantic
  carriers. They are not evidence by themselves and never outrank
  `explicit` or `curated` artefacts from RFC 0014.
- **Summaries** remain RFC 0014 projection artefacts. Theme summaries
  may exist as navigation aids, but they do not replace RFC 0014
  `summary` artefacts or change their trust semantics.

Any implementation that cannot be expressed in RFC 0014 terms is out
of scope for this RFC.

## Proposed design outline

### 1. Source-of-truth boundaries

The hierarchy should preserve RFC 0007's storage split:

- PostgreSQL is authoritative for raw evidence, workspace documents, the
  transactional outbox, and audit history.
- Oxigraph is authoritative for structure, provenance, temporal edges,
  lineage, and retractions.
- Qdrant is authoritative for vector indexes and denormalized serving
  payloads.
- Runtime clustering artefacts are accelerators, not the durable source
  of semantic membership or theme identity.

### 2. Materialized node families

This RFC should define three outline node families.

- **Episode nodes**
  - Represent contiguous conversational or document-revision evidence.
  - Preserve observed ordering and source contiguity.
  - Carry stable identifiers, workspace binding, lifecycle state,
    provenance references, and observed-time fields.

- **Semantic carriers**
  - Represent reusable long-term statements distilled from one or more
    episodes or document spans.
  - Carry support references, confidence, temporal hints, extraction
    mode, and any mapping needed to RFC 0014 `fact`, `concept`, or
    promotable profile material.
  - Must not become retrievable unless their support references resolve
    to concrete evidence.

- **Theme nodes**
  - Represent optional higher-level groupings over semantic carriers for
    navigation and retrieval.
  - Carry stable identifiers, lifecycle state, member references, and
    lineage metadata.
  - Must be treated as derived navigation structures rather than
    evidence or truth claims.

### 3. Projection pipeline

The implementation should follow an additive pipeline:

1. raw evidence arrives from the transactional outbox
2. `memoryd` resolves or creates the current draft episode
3. boundary detection either extends the current episode or finalizes it
4. finalized episodes are summarized, embedded, and written to durable
   stores
5. semantic extraction runs over finalized episodes
6. semantic candidates are validated, deduplicated, and upserted
7. optional theme assignment runs after semantic acceptance
8. summary refresh and maintenance work run asynchronously

This RFC should define the invariants of each stage, not the exact
models or prompts.

### 4. Boundary detection

Boundary detection should remain pluggable.

- A structured large language model (LLM) classifier may propose a new
  episode boundary.
- An encoder-based classifier may propose a new boundary from
  embeddings, time gaps, and lexical shift.
- Hard split rules must take precedence. At minimum these include a new
  conversation identifier, source-kind change, explicit operator
  markers, hard time gaps, and document revision changes.

The key compatibility point with RFC 0014 is that boundary detection
defines evidence packaging, not truth semantics.

### 5. Temporal model

This RFC should formalize two time dimensions:

- **Observed time** for when evidence appeared in the source stream.
- **Valid time** for when a semantic claim is meant to hold.

At minimum, the hierarchy should preserve:

- episode observed start and end
- semantic valid-from and valid-to
- an explicit `temporal_basis` such as `explicit`, `metadata`,
  `inferred`, or `unknown`
- graph edges for precedence, overlap, and supersession

Model-inferred valid time must remain lower-trust than metadata-backed
or curated time unless corroborated, consistent with RFC 0014's
promotion rules.

### 6. Provenance model

Every retrievable semantic carrier and every theme must retain a
durable evidence chain.

The implementation should support:

- derivation edges from semantic carriers to episodes
- derivation edges from episodes to messages or document spans
- support edges to specific evidence references
- membership edges from semantic carriers to themes

Theme nodes are never evidence by themselves. They inherit provenance
only through their members.

### 7. Curated-document projection

Curated workspace documents such as `MEMORY.md` should be projectable as
synthetic document-revision episodes.

This design should preserve two rules:

- document-derived material can skip conversational boundary detection
  because the revision already defines a stable block
- curated support should outrank weak inferred support during
  deduplication and contradiction handling, unless explicitly retracted

This is compatible with RFC 0014's distinction between `explicit`,
`curated`, and inferred artefacts.

### 8. Retraction and purge propagation

Retraction should remain soft by default and propagate through the
hierarchy.

At minimum:

- retracting raw evidence should retract unsupported episodes
- retracting episodes should retract unsupported semantic carriers
- retracting all active members may retract a theme
- workspace purge must remove all hierarchy artefacts, including draft
  episodes and checkpoints

Retracted nodes should remain auditable until purge or compaction
policy removes them.

### 9. Rollout outline

The recommended rollout is staged:

1. shadow projection of episodes and semantic carriers
2. provenance hardening and unresolved-support rejection
3. optional theme shadowing and backfill
4. retrieval integration behind a profile or feature flag

This keeps the materialization hierarchy additive while RFC 0014
semantics remain stable.

## Open questions

- Should semantic carriers be a single internal structure, or should
  separate carrier shapes exist for fact-like, concept-like, and
  profile-candidate material?
- Should theme summaries be stored as dedicated artefacts, or generated
  from theme membership on demand?
- Should retraction trigger summary regeneration immediately, or mark
  summaries stale for asynchronous refresh?
- How much of theme balancing should remain runtime policy rather than fixed
  by RFC 0016 and ADR 003?

## Recommendation

Adopt this RFC as the implementation-facing sibling to RFC 0014.
RFC 0014 should remain the authority on memory semantics and trust
rules. RFC 0015 should become the authority on how `memoryd`
materializes those semantics into durable hierarchical structures for
evidence preservation, navigation, and future retrieval work.
