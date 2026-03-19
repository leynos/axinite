# RFC 0016: Theme detection and sparsity rebalancing for memoryd

## Preamble

- **RFC number:** 0016
- **Status:** Proposed
- **Created:** 2026-03-18
- **Depends on:** RFC 0015 and ADR 003
- **Related:** RFC 0017 and ADR 005

## Summary

This RFC introduces a `ThemeManager` inside `memoryd` that organizes accepted
semantic carriers into durable, workspace-local themes using Chutoro as the
clustering substrate.[^1][^2][^3] The manager owns stable theme identifiers,
attach, split, and merge policy, lineage, k-nearest neighbour (kNN)
navigation, and an implementation-friendly sparsity-balancing objective
inspired by xMemory.[^3]

The key design rule is simple: Chutoro proposes cluster structure, but
`memoryd` owns theme identity and balancing policy. This keeps Chutoro useful
as a generic density-based clustering engine while allowing `memoryd` to apply
retrieval-aware rules such as stable IDs, provenance-preserving reassignments,
curated-memory precedence, and workspace-local cooling-off windows.

## Problem

After RFC 0015, `memoryd` can materialize episodes, semantic carriers, and
optional theme nodes, but a growing semantic-carrier collection still suffers
from the same structural problem that motivated xMemory: without a maintained
high-level organization, top-level retrieval collapses into dense local
regions, and near-duplicate carriers keep crowding the same query
neighbourhood.[^1][^3]

A batch clustering pass is not enough on its own. Theme management needs all
of the following:

- stable theme identifiers that survive ordinary attaches and most merges
- lineage for split and merge history
- bounded theme size so routing does not degrade as the workspace grows
- semantically coherent themes that do not become tiny isolated islands
- local maintenance so a single crowded region does not trigger a full rebuild

Chutoro can supply incremental clustering, subset reclustering, snapshots, and
diagnostics, but its cluster labels are not theme IDs and should not be
treated as such.[^2] The missing piece is a controller that turns clustering
output into durable memory structure.

## Current state

RFC 0015 defines semantic carriers and optional theme nodes as materialized
structures owned by `memoryd`, and explicitly limits themes to derived
navigation status rather than a new epistemic class.[^1] ADR 003 places theme
management in `memoryd`, not in Chutoro, so the sidecar owns stable IDs,
lineage, and balancing policy.[^4]

Under this design:

- only accepted semantic carriers with validated support references may enter
  theme management
- theme membership changes must not alter the underlying RFC 0014 projection
  class or epistemic status of any memory artefact
- Chutoro snapshots remain rebuildable accelerators rather than the durable
  source of truth

## Goals and non-goals

- Goals:
  - Introduce a workspace-local `ThemeManager` that owns stable theme IDs.
  - Use Chutoro for bootstrap clustering and local split proposals.
  - Maintain a bounded, queryable theme partition over semantic carriers.
  - Preserve lineage across attaches, splits, merges, retractions, and
    rebuilds.
  - Maintain a theme and semantic-carrier kNN graph for later recall.

- Non-goals:
  - Move theme balancing policy into `chutoro-core`.
  - Require exact decremental clustering for every delete or retract.
  - Define the final query-time retrieval path. That is covered by RFC 0017.
  - Introduce global themes shared across workspaces.

## Compatibility with RFC 0014 and RFC 0015

This RFC does not introduce a sixth memory class.

- Themes remain derived navigation structures from RFC 0015, not evidence by
  themselves.[^1]
- Theme membership does not rewrite the RFC 0014 projection class,
  epistemic status, or contradiction state of the underlying artefacts.[^5]
- Theme summaries are navigation aids. They do not outrank curated facts,
  profiles, or explicit episodes from RFC 0014.[^5]

Any implementation that treats theme identity as a substitute for provenance or
promotion is out of scope.

## Proposed design

### Theme manager responsibilities

`ThemeManager` is a sidecar-local service, instantiated once per workspace. It
owns:

- the current Chutoro session over active semantic-carrier vectors
- the current theme records and lineage state
- the high-level kNN graph over themes and semantic carriers
- the balancing policy and thresholds
- asynchronous jobs for attach, split, merge, summary refresh, and compaction

The authoritative membership edges live in Oxigraph and are denormalized into
Qdrant payload for serving. Chutoro snapshots are acceleration artefacts. If a
snapshot is lost or corrupted, the manager rebuilds from active semantic
carriers and their stored embeddings.

### Bootstrap policy

A workspace starts in one of two modes:

- **pre-bootstrap:** too few active semantic carriers exist to form meaningful
  themes
- **active themes:** enough semantic carriers exist for clustering and
  balancing

During pre-bootstrap, each semantic carrier either becomes a singleton theme
or enters a short-lived pending queue. Once `bootstrap_min_semantics` is
reached, the manager runs a batch Chutoro clustering pass over all active
semantic carriers and materializes theme records from the result.

Bootstrap must produce:

- a stable `theme_id` per accepted cluster
- a medoid or exemplar semantic carrier per theme
- a centroid vector computed in `memoryd` over the embedding space
- a title and summary
- lineage roots for later split and merge events

Noise points from Chutoro are not discarded. Each noise semantic carrier
becomes either a singleton theme or a pending carrier awaiting the next
bootstrap window.

### Incremental attach path

New semantic carriers do not trigger a full recluster. Instead, the manager
performs a local attach procedure:

1. retrieve candidate themes by nearest theme vector and theme-kNN traversal
2. score each candidate theme by similarity and expected objective change
3. attach to the best theme if the resulting structure improves or preserves
   the balancing objective within the configured tolerance
4. otherwise create a new singleton theme

Attach updates:

- the `semantic_carrier -> theme` membership edge
- denormalized member counts in Qdrant payload
- the theme centroid and medoid candidate set
- the theme summary refresh queue
- affected theme and semantic-carrier kNN edges

Attach is synchronous enough to keep the structure usable between maintenance
cycles, but expensive summary refresh stays asynchronous.

### Balancing objective

This RFC adopts an implementation-friendly approximation of xMemory's
sparsity-semantics objective.[^3]

```plaintext
score(P) =
    w_sparse   * (1 - Σ_t (|t| / N)^2)
  + w_cohesion * mean_i cos(e_i, centroid(theme(i)))
  + w_neigh    * mean_t bell(nn_sim(t), μ, σ)

bell(x, μ, σ) = exp(-((x - μ)^2) / (2σ^2))
```

Where:

- `P` is the partition of active semantic carriers into themes
- `N` is the number of active semantic carriers
- `e_i` is the embedding of semantic carrier `i`
- `nn_sim(t)` is the nearest-neighbour similarity of theme `t`
- `μ` and `σ` are the rolling median and median absolute deviation of
  theme-neighbour similarities

The first term rewards balanced theme sizes by penalizing large dense buckets.
The second rewards within-theme semantic cohesion. The third discourages both
near-duplicate themes and isolated semantic islands.

This objective is evaluated on local proposals, not on every query. The
controller is therefore free to use a more practical approximation than the
paper's exact notation, so long as the behaviour matches the intended trade-
off between dense collapse and over-fragmentation.

### Default policy values

All thresholds are configuration, not invariants. The recommended starting
defaults are:

- `max_semantics_per_theme = 12`
- `min_semantics_per_theme = 3`
- `theme_knn_k = 10`
- `bootstrap_min_semantics = 24`
- `split_cooldown = 1h`
- `merge_cooldown = 1h`

The size cap of `12` is the notable one. xMemory's appendix reports the best
average LoCoMo F1 at that cap relative to `14`, `10`, and `8`, so it is a
reasonable shadow-mode default rather than a mystical number from the heavens.
[^3]

### Split proposals

A theme enters split evaluation when any of the following hold:

- member count exceeds `max_semantics_per_theme`
- cohesion drops below a configured floor
- the local objective falls far enough below the workspace median
- a burst of new semantic carriers lands in the same region within one
  cooldown window

Split evaluation uses Chutoro's local reclustering capability over the theme's
member semantic carriers or a wider subset including near neighbours.[^2] The
manager generates candidate partitions, scores them with the balancing
objective, and accepts the best proposal if all accepted children satisfy the
minimum-size rules or are explicitly marked singleton.

Split rules:

- a split creates new theme IDs
- the source theme is marked `superseded`
- lineage edges record `split_from`
- semantic carriers keep their IDs and only their membership edges change
- all provenance and retraction history stays attached to semantic carriers,
  not themes

### Merge proposals

A theme enters merge evaluation when any of the following hold:

- member count drops below `min_semantics_per_theme`
- a neighbour theme becomes too similar in centroid space
- repeated attach decisions keep routing new semantic carriers to both themes
  with near identical scores
- retractions leave a theme small but semantically redundant

Merge evaluation considers a small set of nearest neighbour themes from the
theme-kNN graph. Each candidate merge is scored locally. The manager accepts
the best merge that improves the objective and does not violate safety rules.

Merge rules:

- if one theme is clearly dominant, its `theme_id` survives and absorbs the
  smaller theme
- otherwise a new merged theme ID is created and both sources become
  `superseded`
- lineage edges record `merged_from`
- summaries and centroid vectors refresh asynchronously

### Stable identity and lineage

Theme IDs are sidecar-owned UUIDs, not Chutoro labels. This gives the manager
a stable handle even when Chutoro snapshots change cluster numbering or local
reclustering produces different label assignments.

Required lineage events:

- `theme.created`
- `theme.attached`
- `theme.split`
- `theme.merged`
- `theme.retracted`
- `theme.rebuilt`

Each event records:

- workspace ID
- snapshot version, if one exists
- semantic carrier IDs moved
- prior and new theme IDs
- objective score before and after
- trigger reason

This event log is the audit trail for later browseability and operator trust.

### Theme and semantic-carrier kNN graph

The manager maintains two sparse neighbour graphs:

- a theme-level graph over theme centroids
- a semantic-carrier-level graph over active vectors

Authoritative edges are stored in Oxigraph. Qdrant payload may denormalize the
top-k neighbour IDs and similarities for fast serving. The graph is updated:

- after attach
- after split or merge
- after large retraction batches
- during periodic rebuild or compaction

The kNN graph is deliberately sparse because later recall only needs high-
level navigation, not a quadratic museum of every possible relationship.[^3]

### Summary refresh and compaction

Theme summaries are derived artefacts. They are refreshed when:

- the medoid changes
- member count changes materially
- a split or merge completes
- curated semantic carriers enter or leave the theme

Compaction is needed because Chutoro's live session is append-friendly, not
magically free forever. The manager therefore rebuilds the session from active
semantic carriers when either of these triggers fire:

- active-to-retracted ratio exceeds a configured threshold
- checkpoint age or snapshot drift exceeds a configured threshold

Compaction does not change theme IDs by itself. It only rebuilds the
clustering substrate and recomputes diagnostics.

### Background jobs

The following Apalis jobs are added to `memoryd`:

- `AttachSemanticCarrier`
- `EvaluateThemeSplit`
- `EvaluateThemeMerge`
- `RefreshThemeSummary`
- `RefreshThemeGraph`
- `CompactThemeSession`
- `RebuildWorkspaceThemes`

These jobs must be idempotent by workspace and target node IDs. A failed split
or merge proposal must leave the current active partition untouched.

## Requirements

### Functional requirements

- Each workspace must have its own independent theme manager and balancing
  policy state.
- Every active semantic carrier must belong to zero or one active theme.
- Theme IDs must remain stable across attach operations and most merges.
- Split and merge history must be auditable after the fact.
- The manager must be able to rebuild from active semantic carriers without
  relying on old Chutoro label IDs.

### Technical requirements

- The controller must treat Chutoro cluster labels as proposals, not as theme
  identity.
- Local split evaluation must use subset reclustering rather than full
  workspace reclustering by default.
- Theme-kNN maintenance must remain sparse and bounded by configuration.
- Checkpoints and snapshots must be purgeable per workspace.
- Summary refresh and compaction must be asynchronous.

### Operational requirements

- The manager must emit metrics for theme count, average size, p95 size,
  singleton ratio, split rate, merge rate, and rebuild rate.
- Shadow mode must report objective-score drift and theme-size histograms
  before hierarchical recall is enabled.
- Operators must be able to disable split and merge independently for
  diagnostics.

## Compatibility and migration

The recommended rollout is:

1. **Bootstrap only**
   - create theme records from existing active semantic carriers
   - keep split and merge disabled
2. **Attach active**
   - attach new semantic carriers incrementally
   - refresh summaries and graphs
3. **Split shadow**
   - evaluate split proposals and log them without applying them
4. **Merge shadow**
   - evaluate merge proposals and log them without applying them
5. **Full balancing**
   - apply both split and merge
   - feed the resulting structure into RFC 0017 recall

This rollout mirrors the xMemory lesson that restructuring helps, but lets
shadow metrics catch silly thresholds before they mutate the live structure.

## Alternatives considered

### Use Chutoro cluster labels as theme IDs

This is tempting because it is cheap, but it couples theme identity to a
rebuildable clustering session. Snapshot relabeling would become operator
confusion on a stick. Stable IDs and lineage belong in `memoryd`.

### Full workspace reclustering on every maintenance cycle

This keeps policy simple, but it is operationally noisy and wastes the
incremental capabilities already planned for Chutoro. Local maintenance is the
whole point.

### Graph-community detection without Chutoro

A graph-only approach can propose themes, but it gives away the density-based
clustering machinery already being built and makes subset diagnostics harder.
It also tempts the design toward query-time structure rather than maintained
structure.

## Open questions

- Should the manager prefer `recluster_cluster(theme_id)` or a wider
  `recluster_subset` window when a theme sits on a fuzzy boundary with two
  neighbours?
- Should singleton themes be visible to all retrieval profiles, or only to an
  evidence-maximizing profile?
- Should a large curated theme be exempt from the usual size cap, or should
  curated semantic carriers still be balanced like inferred ones?

## Recommendation

Adopt this RFC and keep theme management in `memoryd` as a controller around
Chutoro. This gives the system the plasticity of xMemory without turning the
clustering crate into a policy swamp.

## References

[^1]: RFC 0015: Hierarchical memory materialization for `memoryd`. See
    `docs/rfcs/0015-hierarchical-memory-materialization-for-memoryd.md`.
[^2]: Chutoro design document, section 12: incremental clustering. See
    <https://raw.githubusercontent.com/leynos/chutoro/main/docs/chutoro-design.md>.
[^3]: Beyond RAG for Agent Memory: Retrieval by Decoupling and Aggregation.
    See <https://arxiv.org/abs/2602.02007>.
[^4]: ADR 003: Theme management belongs in `memoryd`. See
    `docs/adr-003-theme-management-belongs-in-memoryd.md`.
[^5]: RFC 0014: Memory projection tiers and promotion rules. See
    `docs/rfcs/0014-memory-projection-tiers-and-promotion-rules.md`.
