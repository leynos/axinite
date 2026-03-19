<!-- markdownlint-disable-next-line MD013 -->
# Architectural decision record (ADR) 004: Dual-path semantic extraction with validated provenance

## Status

Proposed.

## Date

2026-03-18

## Context and problem statement

`memoryd` needs to extract episode nodes, semantic carriers, and theme
summaries from raw messages and document revisions. RFC 0015 defines the
materialization hierarchy, while RFC 0014 defines the epistemic and promotion
rules that accepted outputs must respect.[^1][^2]

A purely large language model (LLM)-based design is expressive, but it can
hallucinate support links. A purely encoder-based design is cheaper and easier
to bound, but it struggles to produce rich canonical statements and high-
quality theme summaries. The decision is therefore not simply about model
preference; it is about how to obtain useful abstractions without ever losing
the evidence trail.

## Decision drivers

- Every retrievable semantic carrier must resolve to concrete evidence
- Local-only execution and bounded fallback paths
- Support for both rich and cheap deployment modes
- Retractable and auditable memory projection
- A shared schema so downstream code does not branch on model family

## Requirements

### Functional requirements

- Both extractor paths must emit the same logical fields: canonical text or
  extractive text, semantic kind, support references, confidence, temporal
  hints, and extraction mode.
- No semantic carrier may become authoritative unless all support references
  validate.
- Theme and episode summary generation must degrade gracefully when the LLM
  path is unavailable.

### Technical requirements

- The extraction interface must remain model-agnostic from the point of view
  of the consolidation pipeline.
- Support references must be structural, not free-form quotations.
- The encoder path must be able to operate without a generative model.

## Options considered

<!-- markdownlint-disable MD013 -->
| Option | Provenance reliability | Quality | Cost | Fallback behaviour |
| --- | --- | --- | --- | --- |
| Option A: LLM-only structured extraction | Weak unless support references are validated strictly, because the model can hallucinate polished but unsupported claims. | Rich canonical statements, temporal inference, and high-quality summaries. | Higher local generative-model cost. | Poor: hierarchical extraction depends on one generative path. |
| Option B: Bidirectional-encoder-only extractive projection | Strong because the pipeline works directly over spans and support references stay structural. | Lower abstraction quality; extractive text is clunky and theme summaries become template-driven. | Cheaper and easier to bound. | Strong bounded fallback, but no richer abstraction path. |
| Option C: Dual-path extraction with shared schema and validated support | Strong once both paths pass the same support-reference validator before promotion. | Rich when the LLM path is available, with encoder fallback for stricter baseline and shadow comparison. | Mixed: supports cheap mode and richer local mode behind one schema. | Strong: encoder path remains available for cheap mode, bounded fallback, and shadow runs. |
<!-- markdownlint-enable MD013 -->

_Table 1: Comparison of extraction options across provenance,
quality, cost, and fallback behaviour._

## Decision outcome / proposed direction

Choose **Option C**.

`memoryd` will implement a shared extraction schema and two extractor
implementations:

- `llm_structured`
- `encoder_extractive`

Both implementations must emit structural support references such as:

- message identifiers and ordinals
- episode sentence ordinals
- document-span references

A semantic carrier becomes authoritative only after the support references
resolve successfully against stored evidence. Unresolved outputs remain
diagnostics, not retrievable memory.

## Goals and non-goals

- Goals:
  - Support rich local extraction when an LLM is available.
  - Preserve a cheap and bounded fallback path.
  - Make provenance validation non-optional.
- Non-goals:
  - Force both extractors to produce identical wording.
  - Require the encoder path to write fluent abstractive prose.

## Migration plan

1. Define the shared extraction schema and support-reference validator.
2. Implement the encoder path first, because it provides the stricter baseline
   for provenance.
3. Add the LLM path with structured outputs and the same validator.
4. Run both paths in shadow mode on the same episodes and compare accepted
   versus rejected outputs.
5. Promote the LLM path to default where local capacity exists, while keeping
   the encoder path as fallback and cheap-mode primary.

## Known risks and limitations

- The two paths may disagree on canonical wording or semantic kind.
- Encoder-only theme summaries will be less elegant than LLM-generated ones.
- Strict provenance validation may reject some otherwise useful LLM outputs.

## Outstanding decisions

- The exact sentence-segmentation rules for encoder provenance remain open.
- The threshold for promoting extractive encoder output to `stable` status is
  still a policy question.

## Architectural rationale

The dual-path design keeps the consolidation pipeline honest. It allows richer
abstraction without treating model output as magic dust, and it ensures that
every retrieved semantic carrier can still point back to something that
actually happened in the source evidence.

## References

[^1]: RFC 0015: Hierarchical memory materialization for `memoryd`. See
    `docs/rfcs/0015-hierarchical-memory-materialization-for-memoryd.md`.
[^2]: RFC 0014: Memory projection tiers and promotion rules. See
    `docs/rfcs/0014-memory-projection-tiers-and-promotion-rules.md`.
