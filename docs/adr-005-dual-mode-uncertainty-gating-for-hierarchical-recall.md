<!-- markdownlint-disable-next-line MD013 -->
# Architectural decision record (ADR) 005: Dual-mode uncertainty gating for hierarchical recall

## Status

Proposed.

## Date

2026-03-18.

## Context and problem statement

xMemory's retrieval design expands from high-level structure down to episodes
and raw messages only when the extra detail reduces the reader's uncertainty.
[^1] That idea is attractive, but practical deployments run into an awkward
engineering fact: not every local model exposes calibrated uncertainty, token
log-probabilities, or even stable enough self-reports to make the gating
signal trustworthy on its own.

A mandatory model-led gate would produce the richest behaviour, but it would
also make hierarchical recall unavailable in cheap or air-gapped modes. A
heuristic-only gate would be simpler, but it would leave quality on the floor
where a judge model is available. RFC 0017 therefore needs a gating contract
that works with and without a judge model.[^2]

The decision is how to make uncertainty-gated expansion feasible without
turning it into all-or-nothing wizardry.

## Decision drivers

- Compatibility with both cheap and rich local deployments
- Bounded latency for `Recall`
- Explainability of expansion decisions
- Ability to shadow-compare proxy and model-led behaviour
- No dependency on a specific model family's application programming
  interface (API) quirks

## Requirements

### Functional requirements

- Hierarchical recall must be able to expand episodes without requiring a
  judge model.
- When a judge model is available, recall should be able to use it to improve
  expansion quality.
- Every accepted or rejected expansion should expose a reason code for shadow
  analysis.

### Technical requirements

- The gating interface must return a normalized gain estimate regardless of
  implementation.
- The proxy path must not require a generative model.
- The model-assisted path must remain local to the sidecar architecture.

## Options considered

### Option A: Mandatory model-assisted uncertainty gating

This option uses a local reader or judge model for every stage-II expansion
decision. It best matches the xMemory paper's spirit, but it couples recall to
local model availability, higher latency, and whatever uncertainty surface
that model exposes.

### Option B: Proxy-only gating

This option uses a deterministic score from features such as semantic support
density, novelty against already selected evidence, temporal fit,
reinforcement, and token cost. It is portable and cheap, but it will
inevitably miss cases where a local judge model can tell that an extra episode
materially changes the likely answer.

### Option C: Dual-mode gating with a shared gain interface

This option defines one interface for stage-II gain estimation and allows two
implementations: model-assisted and proxy-based. Retrieval profiles choose
which one to use, and shadow mode can compare them on the same workloads.

## Decision outcome / proposed direction

Choose **Option C**.

`memoryd` will define a shared stage-II gating interface returning:

- `estimated_gain`
- `estimated_token_cost`
- `reason_code`

Two implementations will sit behind that interface:

- **model-assisted gate**
  - uses a local judge model or log-probability-capable reader when
    available;
- **proxy gate**
  - uses a deterministic score from novelty, support density, temporal fit,
    reinforcement, and token cost.

The proxy gate is the minimum guaranteed path. The model-assisted gate is an
optional improvement path.

## Goals and non-goals

- Goals:
  - Keep hierarchical recall feasible in all deployments.
  - Preserve a higher-quality path where a local judge model exists.
  - Make expansion decisions inspectable rather than mystical.
- Non-goals:
  - Require calibrated uncertainty from every supported model.
  - Promise that the model-assisted gate is always correct.

## Migration plan

1. Implement the proxy gate first and use it in `cheap_v2`.
2. Add the model-assisted gate behind `evidence_v2`.
3. Compare both modes in shadow runs and record disagreement rates.
4. Promote the richer gate only where latency and model availability are
   acceptable.

## Known risks and limitations

- The proxy gate can be too conservative on subtle multi-hop questions.
- The model-assisted gate can be noisy or slow depending on the local model.
- Disagreement between gates is expected and must be analysed rather than
  swept under the rug.

## Outstanding decisions

- The exact proxy-feature weighting remains a tuning question.
- The preferred judge prompt format for the model-assisted path remains open.

## Architectural rationale

This decision keeps the xMemory idea intact without making the entire read
path dependent on one fragile uncertainty source. It gives `memoryd` a sane
default, a richer optional path, and a clean place to compare them.

## References

[^1]: Beyond RAG for Agent Memory: Retrieval by Decoupling and Aggregation.
    See <https://arxiv.org/abs/2602.02007>.
[^2]: RFC 0017: Hierarchical recall for `memoryd`. See
    `docs/rfcs/0017-hierarchical-recall-for-memoryd.md`.
