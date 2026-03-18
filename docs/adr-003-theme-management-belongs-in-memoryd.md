# Architectural decision record (ADR) 003: Theme management belongs in memoryd

## Status

Proposed.

## Date

2026-03-18.

## Context and problem statement

RFC 0015 allows `memoryd` to materialize optional theme nodes as derived
navigation structures over semantic carriers.[^1] RFC 0016 then needs a
practical controller that can attach carriers to themes, split or merge
crowded regions, and preserve stable theme identity across ordinary
maintenance.[^2]

Chutoro is the intended clustering substrate and, under the planned work,
provides incremental sessions, snapshots, local reclustering, and diagnostics.
[^3] The unresolved design question is where theme identity and balancing
policy should live.

Three facts shape the decision:

- xMemory's theme layer is more than clustering labels. It includes attach,
  split, merge, balancing, and high-level navigation policy.[^4]
- Chutoro labels are not stable theme IDs and should not become the source of
  truth for browseable memory structure.[^3]
- `memoryd` already owns workspace isolation, provenance, retractions, and
  the retrieval surface from RFC 0007 and RFC 0015.[^1][^5]

The system therefore needs a clean boundary between generic clustering
mechanics and memory-specific control logic.

## Decision drivers

- Stable theme identifiers across ordinary updates
- Workspace-local purge, retraction, and auditability
- Reuse of Chutoro as a generic clustering engine
- Retrieval-aware policy without polluting `chutoro-core`
- Feasible recovery when clustering snapshots are lost or rebuilt

## Requirements

### Functional requirements

- Theme IDs must remain stable enough for operators and later browseability.
- Split and merge history must be inspectable after the fact.
- Theme balancing must be configurable per workspace.
- The system must rebuild from stored semantic carriers even if a clustering
  checkpoint is lost.

### Technical requirements

- The design must use Chutoro for clustering, not reimplement density-based
  clustering from scratch.
- The design must avoid turning Chutoro cluster labels into durable business
  identifiers.
- The source of truth for theme membership must remain purgeable and auditable
  inside `memoryd`.

## Options considered

### Option A: Put theme identity and balancing policy inside Chutoro

This would move the entire theme manager into the clustering crate. The
advantage is a single component for both clustering and theme structure. The
cost is that Chutoro would now need to understand stable IDs, lineage,
workspace isolation, curated-memory precedence, and retrieval policy. That is
too much semantic baggage for a clustering engine.

### Option B: Keep Chutoro as substrate and place the controller in `memoryd`

This option lets Chutoro provide incremental sessions, local reclustering, and
diagnostics, while `memoryd` owns the stable `ThemeId`, balancing thresholds,
lineage, and integration with provenance and recall. It keeps the
responsibilities separate and allows the theme graph to be rebuilt from
semantic carriers if cluster-session artefacts are lost.

### Option C: Avoid Chutoro and implement a custom theme detector in memoryd

This would keep all logic in one service, but it would discard the planned
incremental clustering work and create a second density-based clustering stack
to maintain. That is needless duplicate machinery.

## Decision outcome / proposed direction

Choose **Option B**.

`memoryd` will own the `ThemeManager` controller, the stable theme
identifiers, the balancing policy, the lineage log, and the high-level kNN
graph. Chutoro will remain the clustering substrate used for bootstrap
clustering, local split proposals, and diagnostics.

The controller may store Chutoro snapshots as acceleration artefacts, but the
authoritative membership edges and lineage state remain in `memoryd` stores.

## Goals and non-goals

- Goals:
  - Keep theme identity durable and sidecar-owned.
  - Reuse Chutoro for what it is excellent at: density-based clustering and
    subset reclustering.
  - Preserve workspace-local security and purge behaviour.
- Non-goals:
  - Teach Chutoro about provenance, curated memory, or retrieval profiles.
  - Expose Chutoro labels directly to operators as durable theme identifiers.

## Migration plan

1. Implement the `ThemeManager` adapter in `memoryd`.
2. Persist stable theme IDs and lineage edges in Oxigraph and denormalized
   payload in Qdrant.
3. Add Chutoro snapshot import and rebuild-from-semantic-carriers fallback.
4. Enable split and merge policy only after shadow evaluation.

## Known risks and limitations

- Two layers of state now exist: clustering snapshots and durable theme state.
  The boundary must remain clear.
- Version skew between `memoryd` and Chutoro capabilities must be detected
  early.
- Rebuilds can temporarily change cluster proposals even if theme IDs remain
  stable.

## Outstanding decisions

- The exact checkpoint format for workspace-local Chutoro snapshots remains
  open.
- The acceptable amount of theme-ID churn during full rebuilds remains a
  policy choice.

## Architectural rationale

This decision keeps the clustering substrate reusable while placing memory
policy where the rest of memory policy already lives. It avoids a haunted-house
architecture in which the clustering crate slowly accumulates sidecar-specific
rules that are impossible to untangle later.

## References

[^1]: RFC 0015: Hierarchical memory materialization for `memoryd`. See
    `docs/rfcs/0015-hierarchical-memory-materialization-for-memoryd.md`.
[^2]: RFC 0016: Theme detection and sparsity rebalancing for `memoryd`. See
    `docs/rfcs/0016-theme-detection-and-sparsity-rebalancing-for-memoryd.md`.
[^3]: Chutoro design document, section 12: incremental clustering. See
    <https://raw.githubusercontent.com/leynos/chutoro/main/docs/chutoro-design.md>.
[^4]: Beyond RAG for Agent Memory: Retrieval by Decoupling and Aggregation.
    See <https://arxiv.org/abs/2602.02007>.
[^5]: RFC 0007: Secure memory sidecar design for axinite. See
    `docs/rfcs/0007-secure-memory-sidecar-design.md`.
