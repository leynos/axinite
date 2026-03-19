# RFC 0013: Auxiliary provider profiles and stable-prefix prompt assembly

## Preamble

- **RFC number:** 0013
- **Status:** Proposed
- **Created:** 2026-03-15

## Summary

Axinite should generalize its current provider chain into named
profiles such as `main`, `auxiliary`, `vision`, and `compression`, then
restructure prompt assembly so the stable policy/tool/context prefix
remains byte-stable across a job or session while volatile turn data
sits after the cache break.

Axinite already supports multiple backends and a decorated provider
chain with retry, smart routing, failover, circuit breaking, caching,
and an optional cheap model. [^1] However, the cheap-model path is
currently centred on the NEAR AI branch and does not encode capability
or privacy constraints for auxiliary workloads. [^1] The smart routing
system provides a 13-dimension complexity scorer that maps to four
tiers (flash, standard, pro, frontier), but those tiers govern
conversational routing rather than auxiliary task dispatch. [^2]

The Hermes Agent analysis, Anthropic's prompt caching guidance, and
OpenAI's prompt caching guidance all point at the same truth: stable
prefixes and separated auxiliary work reduce latency, cost variance,
and feature-coupling failures. [^3] [^4] [^5]

## Problem

### Auxiliary provider gap

Axinite's existing `create_cheap_llm_provider` mechanism constructs a
dedicated provider for heartbeat, evaluation, and routing tasks, but it
is documented as NEAR AI–only and framed narrowly. [^1] Other auxiliary
workloads — summarization/compaction, lightweight classification,
vision analysis, web extraction — currently use the main provider by
default. This creates three problems:

- **Feature-coupling failures**: a provider that handles conversational
  reasoning well may not support vision, or may silently drop
  parameters. Hermes Agent explicitly documents an auto-provider
  fallback chain for vision ("tries OpenRouter → Nous → Codex"). [^3]
- **Cost inefficiency**: auxiliary tasks (summarization, classification)
  do not need frontier-tier models. Running them on the main provider
  wastes budget.
- **Policy mismatch**: some providers retain data in ways the operator
  wants to avoid for auxiliary workloads. Hermes exposes OpenRouter
  routing knobs for `data_collection: "deny"` and
  `require_parameters: true`. [^3]

### Prompt cache instability

Modern large language model (LLM) providers offer significant cost and
latency reductions through prompt prefix caching:

- **OpenAI**: automatic caching with a 50% discount on cached input
  tokens (90% for newer models). Minimum 1 024 cached tokens. [^4]
- **Anthropic**: explicit `cache_control` breakpoints. Cached content
  is billed at a reduced rate with up to 80% latency reduction. [^5]

Both providers require the prompt prefix to be byte-identical across
requests for cache hits to occur. Axinite already has hooks for
Anthropic prompt caching when cache retention is configured, but the
prompt assembly does not systematically separate stable content from
volatile content. [^1]

Hermes Agent explicitly freezes memory injection at session start to
preserve prefix cache performance and recommends keeping the system
prompt stable across a session. [^3]

## Current state

### Provider chain

The current provider chain applies decorators in a fixed order: [^1]

1. `RetryProvider` (exponential backoff)
2. `SmartRoutingProvider` (cheap/primary split)
3. `FailoverProvider` (fallback model with cooldown)
4. `CircuitBreakerProvider` (fast-fail when degraded)
5. `CachedProvider` (in-memory response cache)
6. Optional `RecordingLlm` (trace capture)

This chain is constructed by a single "source of truth" function that
builds the decorated stack conditionally based on configuration. [^1]

### Smart routing

The smart routing system maps user messages to four tiers based on a
13-dimension complexity scorer, then routes to either the cheap
provider or the primary provider. [^2] This is a request-level routing
decision for conversational messages, not a profile-based dispatch for
auxiliary workloads.

### Chaos tests

Axinite's test suite includes explicit LLM provider chaos tests
covering failover, circuit breaking, retry under flakey/hanging/garbage
providers, multi-provider failover chains, and cooldown behaviour. [^6]
This provides a strong foundation for testing auxiliary provider
profiles.

## Goals and non-goals

- Goals:
  - Define named provider profiles (`main`, `auxiliary`, `vision`,
    `compression`) as a first-class configuration surface.
  - Allow each profile to have its own provider, model, fallback
    chain, and capability/privacy metadata.
  - Restructure prompt assembly to separate stable prefix content
    from volatile turn content.
  - Maximize prompt cache hit rates across providers that support
    prefix caching.
  - Extend chaos tests to cover auxiliary-profile failures.
- Non-goals:
  - Replace the smart routing system. Smart routing governs
    conversational message routing; profiles govern auxiliary task
    dispatch. They are complementary.
  - Build a full provider marketplace or gateway. Axinite's resilience
    is internal and deterministic, not dependent on external routers.
  - Mandate specific models for specific profiles. Profile-to-model
    mappings are operator-configurable.

## Proposed design

### 1. Named provider profiles

Extend the provider configuration to support named profiles:

```yaml
llm:
  profiles:
    main:
      backend: anthropic
      model: claude-sonnet-4-5-latest
    auxiliary:
      backend: anthropic
      model: claude-haiku-4-5-latest
      fallback:
        backend: ollama
        model: llama3.2
    vision:
      backend: openai
      model: gpt-5-mini
      fallback:
        backend: anthropic
        model: claude-sonnet-4-5-latest
      capabilities: [vision]
    compression:
      backend: ollama
      model: llama3.2
```

Each profile receives its own decorated provider chain (retry, failover,
circuit breaker, cache). The profile selection is determined by the
workload type, not by per-request complexity scoring.

### 2. Profile dispatch table

<!-- markdownlint-disable MD013 -->
| Workload | Default profile | Rationale |
| --- | --- | --- |
| Conversational reasoning | `main` (with smart routing) | Primary user-facing workload. |
| Summarization/compaction | `auxiliary` | Does not need frontier reasoning. Cost-sensitive. |
| Lightweight classification | `auxiliary` | Fast, cheap, frequent. |
| Vision analysis | `vision` | Requires modality support. |
| Web extraction/summarization | `auxiliary` | Non-critical, cost-sensitive. |
| Heartbeat/evaluation | `auxiliary` | Already uses cheap provider. |
| Memory extraction (RFC 0007) | `auxiliary` | Background, non-critical. |
| Embedding generation | Dedicated embedding config | Already separate. |
<!-- markdownlint-enable MD013 -->

_Table 1: Default profile dispatch._

### 3. Provider capability and privacy metadata

Each provider definition may carry optional metadata:

```yaml
llm:
  profiles:
    main:
      backend: openai
      model: gpt-5
      metadata:
        supports_vision: true
        supports_function_calling: true
        supports_streaming: true
        data_retention: "30_days"
        data_collection: "allow"
```

The profile dispatch logic uses this metadata to proactively avoid
selecting providers likely to fail or violate operator intent, rather
than learning only from runtime failures. [^3]

### 4. Stable-prefix prompt assembly

Restructure prompt construction into two segments:

**Stable prefix** (cached):

1. System instructions and operator policy.
2. Identity files (`SOUL.md`, `AGENTS.md`, `USER.md`, `IDENTITY.md`).
3. Active skill definitions and tool schemas.
4. Intent contract (RFC 0010).
5. Long-lived workspace context (pinned documents, project metadata).

**Volatile suffix** (not cached):

1. Recent conversation turns.
2. Tool call results from the current iteration.
3. Compaction summaries.
4. Retrieved workspace documents (injected on demand).

The boundary between stable and volatile content is marked with the
provider-appropriate cache control mechanism:

- **Anthropic**: `cache_control: {"type": "ephemeral"}` breakpoint
  after the stable prefix.
- **OpenAI**: stable prefix at request start, relying on automatic
  prefix caching.
- **Other providers**: no explicit caching; the separation still
  benefits prompt structure clarity.

### 5. Prefix freeze scope

The stable prefix should freeze at one of three boundaries:

- **Per-thread**: the prefix is computed when a thread is created and
  remains stable for the thread's lifetime. Workspace changes are not
  reflected until a new thread is created.
- **Per-job**: the prefix is computed when a job is dispatched. Suitable
  for long-running jobs that should not see mid-job workspace changes.
- **Per-provider-session**: the prefix is computed when the provider
  connection is established. For WebSocket Responses (RFC 0008), this
  aligns with the connection lifetime.

The recommended default is per-job, which balances cache stability with
reasonable freshness.

## Requirements

### Functional requirements

- Provider profiles must be configurable via the existing configuration
  system (YAML/environment).
- Each profile must support independent provider, model, fallback chain,
  and metadata.
- Prompt assembly must separate stable prefix from volatile suffix.
- The stable prefix must remain byte-identical across consecutive
  requests within the freeze scope.
- Auxiliary workloads must default to the `auxiliary` profile without
  requiring per-call configuration.

### Technical requirements

- Profile construction must reuse the existing decorated provider chain
  factory (retry, failover, circuit breaker, cache).
- The cache breakpoint must be provider-specific (Anthropic
  `cache_control`, OpenAI automatic caching, etc.).
- Chaos tests must cover auxiliary-profile failure modes (fallback from
  auxiliary to main, circuit breaking on auxiliary provider).
- The profile dispatch table must be configurable, not hardcoded.

## Compatibility and migration

The change is backward-compatible. Existing configurations without
explicit profiles should continue to function by treating the current
provider as both `main` and `auxiliary`. The cheap-model mechanism
becomes a specific case of the `auxiliary` profile.

Migration involves:

1. Adding profile support to the provider configuration parser.
2. Refactoring the provider chain factory to construct per-profile
   chains.
3. Adding profile selection to each auxiliary workload call site.
4. Restructuring prompt assembly to separate stable and volatile
   segments.
5. Extending chaos tests to cover profile-specific failure modes.

## Alternatives considered

### Option A: External router (LiteLLM Proxy)

Delegate multi-provider resilience to an external router such as
LiteLLM Proxy. [^3] Hermes Agent takes this approach. This creates
an operational escape hatch but adds an external dependency and does
not address prompt assembly. Axinite's internal resilience
(retry/failover/circuit breaker) is stronger than relying on an
external router alone.

### Option B: Per-call provider selection

Instead of named profiles, allow each call site to specify a provider
directly. This is more flexible but scatters provider-selection logic
across the codebase and makes it harder to maintain consistent
fallback chains.

## Open questions

- Which operations default to auxiliary profiles — only
  summarization/compaction/classification, or also vision and web
  extraction? The proposed dispatch table treats vision as a separate
  profile because it requires modality support, while web extraction
  uses auxiliary.
- Should provider definitions carry first-class capability and privacy
  metadata so routing can avoid predictable failure or policy mismatch
  before a request is sent? The proposed metadata fields are optional;
  the question is whether they should be required for certain
  workloads.
- Does the stable-prefix snapshot freeze per thread, per job, or per
  provider session? The recommended default is per-job, but
  per-provider-session may be more appropriate for WebSocket
  Responses.
- How should the auxiliary profile interact with smart routing? The
  proposed design treats them as complementary: smart routing governs
  conversational tier selection within the `main` profile, while
  profiles govern workload dispatch.

## Recommendation

Implement named provider profiles as a first-class configuration
surface, starting with `main` and `auxiliary` profiles. Restructure
prompt assembly to separate stable prefix from volatile suffix, with
per-job freeze scope as the default. Add optional capability and
privacy metadata to provider definitions. Extend the chaos-test suite
to cover auxiliary-profile failure modes. Defer `vision` and
`compression` as separate profiles until workloads that require
them are productized.

---

[^1]: LLM provider chain and `create_cheap_llm_provider`. See
    `src/llm/mod.rs`.

[^2]: Smart routing specification. See
    `docs/smart-routing-spec.md`.

[^3]: Hermes Agent analysis, provider resilience and auxiliary models.
    See `docs/Axinite lessons from Hermes Agent on provider resilience
    and sub-agents.md`.

[^4]: OpenAI prompt caching. See
    <https://developers.openai.com/api/docs/guides/prompt-caching/>.

[^5]: Anthropic prompt caching. See
    <https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching>.

[^6]: LLM provider chaos tests. See provider test modules in
    `src/llm/`.
