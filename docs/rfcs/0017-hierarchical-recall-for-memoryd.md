# RFC 0017: Hierarchical recall for memoryd

## Preamble

- **RFC number:** 0017
- **Status:** Proposed
- **Created:** 2026-03-18
- **Depends on:** RFC 0015, RFC 0016, and ADR 005
- **Amends:** RFC 0007 `Recall`
- **Related:** RFC 0014

## Summary

This RFC extends `memoryd`'s `Recall` method from flat retrieval over
projection artefacts into a top-down hierarchical recall path over themes,
semantic carriers, episodes, and optional raw messages.[^1][^2][^3] The design
follows the two-stage shape of xMemory: first select a compact and diverse
high-level skeleton, then expand to finer evidence only when the added detail
materially improves the expected answer quality.[^3]

The sidecar still does not generate the final user answer. `Recall` returns a
context pack, selected node IDs, provenance traces, and diagnostics. Optional
model-assisted scoring is used only to decide whether extra episodes or raw
messages deserve their token cost.

## Problem

RFC 0007's proposed `Recall` method is intentionally small and safe, but it is
still shaped around flat search over retrievable units.[^2] Once `memoryd`
stores episodes, semantic carriers, and themes, flat search leaves value on
the table:

- multi-fact questions need several connected semantics rather than one best
  chunk
- temporal questions need evidence chains, not isolated snippets
- naive expansion from themes or semantic carriers can still flood the prompt
  with redundant episodes
- pruning inside evidence units risks breaking the very temporal links the
  hierarchy was meant to preserve

The xMemory paper's answer is a two-stage recall path: representative
selection over a high-level graph, followed by uncertainty-gated expansion
over intact evidence units.[^3] `memoryd` needs an implementation of that idea
that fits its local-only architecture and its narrow RPC surface.

## Current state

RFC 0015 provides durable episode nodes, semantic carriers, and theme nodes,
together with provenance and temporal edges.[^1] RFC 0016 provides a
maintained theme partition and sparse theme and semantic-carrier kNN
graphs.[^4] RFC 0014 already defines the normative rule that recall should
keep projection classes and epistemic status visible rather than flattening
them into one undifferentiated memory stream.[^5] RFC 0007 already allows
`Recall` over Unix domain socket (UDS) and keeps read scopes separate from
write scopes.[^2]

What is missing is a query-time read path that can:

- use themes and semantic carriers as routing structure rather than as a pile
  of unrelated vectors
- respect token budgets before returning context
- expose enough diagnostics for later shadow evaluation and browseability
- fall back cleanly to flat recall when the hierarchy is unavailable or stale

## Goals and non-goals

- Goals:
  - Extend `Recall` with a hierarchical profile.
  - Select a compact, diverse, query-relevant high-level backbone before
    episode expansion.
  - Expand only to intact episodes or message blocks, never by pruning inside
    them.
  - Return provenance and selection diagnostics for evaluation and operator
    trust.
  - Keep flat recall as a fallback profile.

- Non-goals:
  - Turn `memoryd` into the final answer generator.
  - Make model-assisted uncertainty scoring mandatory for every deployment.
  - Replace the existing `ReadFacts` method.
  - Introduce arbitrary graph traversal outside the hierarchy.

## Compatibility with RFC 0014 and RFC 0015

RFC 0014 remains authoritative for projection-layer semantics, epistemic
status, and hypothesis filtering.[^5] RFC 0015 remains authoritative for the
materialized hierarchy and provenance model.[^1]

This RFC therefore constrains hierarchical recall in four ways:

- profile and fact recall lanes must remain visible, not collapsed into theme
  routing
- theme nodes and semantic carriers are routing structures, not a replacement
  for RFC 0014 projection classes
- every returned context block must retain its projection class and epistemic
  status
- hypotheses and retracted artefacts remain excluded unless the caller
  explicitly asks for them and policy allows them

## Proposed design

### `Recall` profiles

`Recall` remains the same RPC method, but gains a `profile` field. Profiles
are explicit because hierarchical retrieval has more moving parts than flat
search.

Required profiles:

- `flat_v1`: RFC 0007 behaviour or the closest compatible implementation
- `hierarchical_v2`: theme and semantic selection plus bounded episode
  expansion
- `cheap_v2`: hierarchical retrieval with proxy-only expansion gating and no
  raw-message expansion by default
- `evidence_v2`: hierarchical retrieval with model-assisted expansion gating
  and optional raw-message expansion

The existing `memory.recall` capability scope remains sufficient. The method
is already read-only but potentially sensitive, and hierarchical recall does
not change that basic truth.

### Request contract

For illustration, the following pseudo-schema shows the additional request
fields needed by hierarchical recall.

```plaintext
RecallRequest {
  workspace_id: string
  query: string
  profile: "flat_v1" | "hierarchical_v2" | "cheap_v2" | "evidence_v2"
  token_budget: uint32
  allow_message_expansion: bool
  explain: bool
  preferred_source_kinds: repeated string
  time_range: optional TimeRange
  max_themes: optional uint32
  max_semantics: optional uint32
  max_episodes: optional uint32
}
```

If any `max_*` field is absent, the retrieval profile supplies defaults. The
budget is advisory but must be enforced before the response is returned.

### Stage 0: Projection-aware candidate generation

Candidate generation is intentionally simple and fast:

1. embed the query once
2. retrieve profile and fact candidates when the caller profile permits them
3. retrieve nearest themes from the theme collection
4. retrieve nearest semantic carriers from the semantic-carrier collection
5. union the directly retrieved themes with the themes induced by top semantic
   hits
6. apply workspace, lifecycle, scope, retraction, and time filters

This stage is still vector retrieval, but it no longer decides the final
answer context on its own. It only builds the candidate pool for the
structured selection stages.

### Stage I: Representative selection over the high-level graph

Stage I works over the candidate theme and semantic-carrier sets plus their
kNN edges from RFC 0016. The goal is to avoid choosing six near-identical
neighbours when two or three representatives would cover the same region.

The selection procedure is greedy and coverage-aware:

- score each candidate by query relevance and the amount of new neighbourhood
  it covers
- choose the best next representative
- mark its covered neighbours as represented
- stop when the target coverage or the high-level budget is reached

This stage is run twice:

- once over theme candidates to choose the top-level routing skeleton
- once over the induced semantic-carrier candidates inside or near those
  themes

The output of Stage I is therefore a small set of themes and semantic
carriers, not yet the full evidence pack. That skeleton is what later allows a
multi-fact query to stay broad without spraying tokens everywhere.

### Stage II: Episode and message expansion

Stage II gathers candidate episodes from the selected semantic carriers.
Episodes are ranked by a mixture of:

- direct query similarity
- support strength from selected semantic carriers
- temporal fit to the query, if a time filter exists
- reinforcement and recency bonuses from RFC 0007[^2]

The expansion loop is then:

1. assemble a coarse context from selected profile/fact candidates and theme
   plus semantic summaries
2. consider the next episode
3. estimate whether adding it materially improves the expected answer
4. include it only if the gain exceeds the configured threshold and the token
   budget still permits it
5. optionally repeat the same process for raw messages inside already selected
   episodes

The crucial rule is that expansion happens over intact units only. An included
episode remains a contiguous block; an included raw-message block remains a
contiguous slice inside a selected episode. No pruning step may punch holes
through the middle of an evidence chain.

### Uncertainty and gain estimation

Stage II needs a gating signal. The chosen decision is defined in ADR 005:

- use model-assisted uncertainty scoring when the selected profile and local
  runtime allow it
- otherwise use a proxy score based on novelty, support density, temporal fit,
  and token cost

In both cases, the scoring interface returns:

- `estimated_gain`
- `estimated_token_cost`
- `reason_code`

This lets shadow evaluation compare the cheap and evidence-heavy modes without
lying about why the system expanded or stopped.

### Context assembly

The response includes both a structured selection trace and a ready-to-prompt
context pack.

The assembled context is ordered as follows:

1. selected profile traits and curated facts
2. selected theme summaries
3. selected semantic-carrier statements grouped by theme
4. selected episodes in temporal order
5. optional raw-message blocks nested under their parent episodes

This order gives the reader model a compact map before it sees the lower-level
evidence. It is also consistent with RFC 0014's rule that projection class and
epistemic status stay visible instead of disappearing into a flat bucket of
snippets.[^5]

### Response contract

Hierarchical recall returns the usual context plus diagnostics.

Required response fields:

- `context_blocks`
- `selected_theme_ids`
- `selected_semantic_ids`
- `selected_episode_ids`
- `selected_message_refs`, if any
- `provenance_refs`
- `estimated_tokens`
- `fallback_reason`, if the method dropped to `flat_v1`
- `selection_trace`, when `explain = true`

`selection_trace` should include, at minimum:

- profile used
- stage-I coverage ratio
- stage-II accepted and rejected candidates
- gain or certainty deltas
- stop reason

### Fallback behaviour

Hierarchical recall must fail soft, not dramatically and with theatrical
smoke.

Fallback triggers include:

- no active themes exist for the workspace
- the theme graph is stale or internally inconsistent
- expansion scoring is unavailable for the chosen profile
- the hierarchical data store is degraded

When a fallback trigger fires, `memoryd` returns `flat_v1` recall together
with a `fallback_reason`. This keeps the read path available while making the
degradation inspectable.

### Shadow evaluation

Before `hierarchical_v2` becomes the default, shadow mode should compare it
with `flat_v1` on the same golden-set workloads.

Required shadow metrics:

- answer-coverage overlap against gold evidence sets
- token cost per query
- recall latency p50 and p95
- selected-theme count and selected-episode count
- fallback rate
- temporal-consistency errors
- message-expansion acceptance rate

The point is not to worship the metrics as if they descended from the heavens
engraved on stone tablets. The point is to catch obviously bad trade-offs
before the profile becomes operator-visible.

## Requirements

### Functional requirements

- `Recall` must support both flat and hierarchical profiles.
- Hierarchical recall must return enough structure to explain later why a
  theme or episode was chosen.
- Expansion must happen only over intact episodes or contiguous raw-message
  blocks.
- The response must remain within the declared token budget or report clearly
  why it could not.
- Fallback to `flat_v1` must preserve availability.

### Technical requirements

- Query embedding must be computed once per request.
- Stage I must use the stored theme and semantic-carrier kNN graphs rather
  than constructing a fresh global graph per query.
- Stage II must be compatible with both model-assisted and proxy-based
  gating.
- The read path must remain local-only over the existing UDS RPC interface.
- Hierarchical recall must work even when the optional concept collection from
  RFC 0007 is absent.

### Operational and safety requirements

- The sidecar must emit per-profile latency and fallback metrics.
- The `explain` mode must redact any secret-like payload already blocked by
  the RFC 0007 leak detector.[^2]
- Retracted nodes and support references must never appear in the final
  context pack.
- Message expansion must be disabled by default in cheap profiles.

## Compatibility and migration

Recommended rollout:

1. **Read-only shadow**
   - build `hierarchical_v2`, but return `flat_v1` to callers
   - log the hierarchical selection trace and budget use
2. **Cheap profile exposure**
   - expose `cheap_v2` to internal operators
   - keep `flat_v1` as the default
3. **Evidence profile exposure**
   - expose `evidence_v2` where a local judge model is available
4. **Default switch**
   - promote `hierarchical_v2` to the default only after shadow metrics and
     browse traces look sane

Compatibility is otherwise additive. Existing callers can keep using `flat_v1`
without knowing the hierarchy exists.

## Alternatives considered

### Flat search plus pruning

This is the familiar approach, and xMemory's results are precisely why it is
not enough: pruning reduces tokens, but it can still throw away answer-bearing
details and break evidence chains.[^3]

### Graph traversal only, without a theme and semantic candidate stage

A pure graph walk is attractive in theory, but it turns recall into a
navigation problem before relevance is anchored. The candidate stage gives the
procedure a query-focused starting point.

### Always expand to raw messages once an episode is selected

This produces faithful evidence, but it wastes tokens on many queries whose
answers are already supported by themes, semantic carriers, and episode
summaries.

## Open questions

- Should theme summaries be omitted entirely when the caller asks for an
  extremely small budget, or should at least one high-level summary always be
  returned?
- Should the sidecar support profile-specific ordering rules, such as grouping
  by time first instead of theme first for explicitly temporal questions?
- Should `Recall` return the rejected-candidate list only in shadow mode, or
  in all `explain` responses?

## Recommendation

Adopt this RFC and treat hierarchical recall as the natural read-path partner
to RFC 0015 and RFC 0016. The consolidation layer only becomes useful when the
retrieval layer is willing to use the structure it built.

## References

[^1]: RFC 0015: Hierarchical memory materialization for `memoryd`. See
    `docs/rfcs/0015-hierarchical-memory-materialization-for-memoryd.md`.
[^2]: RFC 0007: Secure memory sidecar design for axinite. See
    `docs/rfcs/0007-secure-memory-sidecar-design.md`.
[^3]: Beyond RAG for Agent Memory: Retrieval by Decoupling and Aggregation.
    See <https://arxiv.org/abs/2602.02007>.
[^4]: RFC 0016: Theme detection and sparsity rebalancing for `memoryd`. See
    `docs/rfcs/0016-theme-detection-and-sparsity-rebalancing-for-memoryd.md`.
[^5]: RFC 0014: Memory projection tiers and promotion rules. See
    `docs/rfcs/0014-memory-projection-tiers-and-promotion-rules.md`.
