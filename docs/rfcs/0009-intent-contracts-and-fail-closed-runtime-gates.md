# RFC 0009: Intent contracts and fail-closed runtime gates

## Preamble

- **RFC number:** 0009
- **Status:** Proposed
- **Created:** 2026-03-15

## Summary

Axinite should define a first-class intent contract at workspace, thread,
and job scope that records goal, hard constraints, trusted control-plane
artefacts, allowed tool families, approval thresholds, and prohibited sink
promotions. The runtime should evaluate that contract before tool execution,
memory promotion, delegated-job creation, and other high-risk side effects.
Retrieved workspace text, attachments, MCP resources, and similar material
should enter context as explicitly untrusted data unless promoted into a
curated control-plane artefact.

The core lesson from the intent-engineering analysis is that soft policy is
decorative wallpaper, not enforcement. [^1] Axinite already contains strong
deterministic skill activation (keyword/tag/regex scoring with no LLM
involvement) and tool attenuation (installed skills receive a read-only tool
ceiling), plus a WASM isolation boundary that enforces deny-by-default
capabilities. [^2] [^3] This RFC extends those mechanisms into a coherent
contract model that applies across the full agent lifecycle.

## Problem

Axinite's current safety architecture provides strong individual
mechanisms — `SafetyLayer` sanitization, validation, policy checks,
secret-leak detection, approval gating, and skill-based tool
attenuation — but those mechanisms are not organized around a single,
inspectable statement of intent. [^2] [^3] [^4]

The practical consequences of this gap include:

- **No single artefact declares what a job is allowed to do.** Constraints
  are distributed across skill definitions, safety rules, approval config,
  and the system prompt. An operator cannot diff or audit the effective
  policy for a given job.
- **Retrieved content enters context without explicit trust labelling.**
  Workspace recall, attachments, and MCP resources are wrapped by
  `SafetyLayer` with untrusted-content markers, but the runtime does not
  systematically distinguish between control-plane artefacts (identity
  files, skill definitions) and data-plane content (workspace documents,
  search results). [^4] [^5]
- **Inbound/outbound hooks fail open.** The agent module documents that
  hook errors are logged but processing continues, which means a failed
  gate check silently widens authority rather than blocking progress. [^6]
- **The model routes around soft constraints.** Intent-engineering research
  and practical experience consistently show that any non-enforced policy
  is treated as optional in complex scenarios. [^1] OWASP LLM06:2025
  (Excessive Agency) explicitly calls out this failure mode. [^7]

## Current state

### Existing enforcement mechanisms

Axinite already has multiple gate points where deterministic enforcement
can live:

- **Tool execution preflight**: `SafetyLayer` validates tool calls before
  execution, applying sanitization, policy checks, and secret-leak
  detection. [^4]
- **Approval gating**: tool calls may require explicit user approval based
  on `ApprovalRequirement` and `ApprovalContext`. [^3]
- **Skill-based tool attenuation**: active skills can restrict the visible
  tool list based on trust level. Installed skills receive a read-only tool
  ceiling; the model cannot invoke tools it cannot see. [^2]
- **WASM capability model**: WASM tools operate under deny-by-default
  capabilities (workspace read prefixes, HTTP allowlists, tool invocation
  aliases, secret existence checks). [^3]
- **Routine guardrails**: lightweight routines enforce a hard iteration cap,
  sequential-only tool execution, and `ApprovalRequirement::Never`-only
  tools. [^6]

### Existing trust surfaces

Identity files (`SOUL.md`, `AGENTS.md`, `USER.md`, `IDENTITY.md`) are
injected into the system prompt and treated as part of the prompt
surface. [^5] The workspace seeded structure encourages writing things down
because mental notes do not survive restarts. [^5] However, no formal
distinction exists between these control-plane artefacts and general
workspace documents at the runtime level.

## Goals and non-goals

- Goals:
  - Define a machine-readable intent contract schema that captures goal,
    constraints, allowed tool families, approval thresholds, trusted
    artefacts, and prohibited sink promotions.
  - Support contract scoping at workspace, thread, and job levels with
    clear precedence rules.
  - Enforce the contract at every gate point: tool execution, memory
    promotion, delegated-job creation, and approval boundary transitions.
  - Treat all retrieved content as untrusted by default unless it has been
    promoted to a curated control-plane artefact.
  - Ensure gate-check failures are fail-closed by default, with explicit
    downgrade-to-approval as an opt-in alternative.
  - Make the effective contract inspectable, diffable, and auditable.
- Non-goals:
  - Replace Axinite's existing safety mechanisms. This RFC composes them
    under a contract, not replaces them.
  - Define the policy language. That is the subject of ADR 001, which
    recommends OPA Rego compiled to WebAssembly.
  - Specify memory projection semantics. That is the subject of RFC 0013.

## Proposed design

### 1. Intent contract schema

An intent contract is a structured document (YAML or JSON) that declares:

| Field | Type | Description |
| --- | --- | --- |
| `goal` | string | Natural-language statement of what the job or session should accomplish. |
| `constraints` | list | Hard constraints that must hold throughout execution. |
| `allowed_tool_families` | list | Allowlisted tool families or specific tool identifiers. |
| `denied_tool_families` | list | Explicitly denied tool families. Takes precedence over allowed. |
| `approval_thresholds` | map | Per-action-class approval requirements (`auto`, `explicit`, `deny`). |
| `trusted_artefacts` | list | Paths to control-plane artefacts trusted as instruction sources. |
| `prohibited_sink_promotions` | list | Sinks into which secret-derived or remote-derived values must not flow without explicit approval. |
| `max_delegation_depth` | integer | Maximum depth of delegated child-job chains. |
| `scope` | enum | `workspace`, `thread`, or `job`. |

_Table 1: Intent contract fields._

### 2. Contract scoping and precedence

Contracts may exist at three levels:

- **Workspace-level**: stored in a canonical path such as
  `context/intent.yml` or as workspace metadata. Provides baseline
  constraints for all threads and jobs in the workspace.
- **Thread-level**: stored as thread metadata. Narrows workspace-level
  constraints for a specific conversation.
- **Job-level**: stored with job metadata in the scheduler's job context.
  Narrows thread-level constraints for a specific job.

Precedence follows the principle of narrowing only: a child scope may
restrict but never widen the parent scope's constraints. If a job-level
contract attempts to allow a tool family denied at workspace level, the
workspace-level denial takes precedence.

### 3. Trust labelling for retrieved content

All content entering the model context should carry an explicit trust
label:

- **Trusted control-plane**: identity files, skill definitions, and
  artefacts explicitly listed in `trusted_artefacts`. These may contain
  instructions.
- **Curated data-plane**: workspace documents promoted by an operator or
  an explicit promotion action. These are treated as reference material but
  not as instruction sources.
- **Untrusted data-plane**: workspace recall results, attachments, MCP
  resources, tool outputs, and any other retrieved content. These carry
  the existing `SafetyLayer` untrusted-content wrapper and are explicitly
  labelled as non-instructional. [^4]

This labelling addresses the indirect prompt injection risk identified in
both the intent-engineering analysis and OWASP LLM01:2025 (Prompt
Injection): when an agent retrieves external or stored text and places it
into the model context, attackers can smuggle instructions that hijack
behaviour. [^1] [^7] [^8]

### 4. Gate evaluation

The runtime evaluates the intent contract at the following gate points:

- **Pre-tool-execution**: before every tool call, the runtime checks
  whether the tool is in the allowed set, whether the call parameters
  satisfy constraints, and whether approval requirements are met.
- **Pre-memory-promotion**: before a memory artefact is promoted from
  `untrusted` to `curated` or from `hypothesized` to `fact` (per
  RFC 0013), the contract is checked.
- **Pre-delegation**: before a child job is created, the runtime verifies
  that the delegation depth does not exceed the contract limit and that the
  child contract narrows rather than widens the parent.
- **Pre-sink-write**: before a value flows into a sink listed in
  `prohibited_sink_promotions`, provenance labels are checked.

Gate evaluation produces a machine-readable decision artefact (see
RFC 0010) that records the contract version, the input, the decision, and
the reason.

### 5. Failure mode: fail-closed with optional downgrade

By default, a gate-check failure denies the action. The contract may
specify per-action-class downgrade behaviour:

- `deny`: action is blocked. No fallback.
- `escalate`: action is blocked and an approval request is raised.
  Execution pauses until the operator responds.
- `log_and_deny`: action is blocked and a warning is emitted, but no
  approval request is raised.

The `auto` approval threshold permits the action without operator
involvement. There is no `log_and_allow` mode: if the gate check fails,
the action does not proceed. This is the fail-closed invariant.

This directly addresses the current fail-open hook behaviour. [^6] Hooks
that cannot be evaluated should block rather than permit. OWASP LLM06:2025
(Excessive Agency) recommends that security controls be enforced
independently from the LLM in a deterministic, auditable manner. [^7]

## Requirements

### Functional requirements

- The intent contract schema must be expressible in YAML and JSON.
- Contracts must be composable across workspace, thread, and job scopes
  with narrowing-only semantics.
- All retrieved content must carry a trust label that the runtime checks
  before incorporating it into model context.
- Gate evaluation must produce a structured decision record for every
  check.
- Gate-check failures must deny the action by default.

### Technical requirements

- Contract evaluation must be synchronous and fast enough to run on the
  critical path of tool execution without perceptible latency.
- The contract schema must be versioned. Schema changes must be
  backward-compatible or explicitly migrated.
- Trust labels must survive serialization/deserialization boundaries,
  including persistence to the database and retrieval.
- The gate evaluation interface must accept structured inputs suitable for
  an external policy engine (see ADR 001).

## Compatibility and migration

The intent contract is additive. Existing workspaces, threads, and jobs
without a contract should continue to operate under the current safety
mechanisms, which become the implicit default contract. Migration involves:

1. Defining a default contract that replicates current behaviour
   (all built-in tools allowed, approval thresholds matching current
   config, identity files as trusted artefacts).
2. Adding contract awareness to the gate points incrementally.
3. Providing a `shadow` mode where contract violations are logged but not
   enforced, allowing operators to validate the contract before switching
   to enforcement.

## Alternatives considered

### Option A: Extend existing safety rules in-place

Incrementally add more regex-based safety rules and approval checks without
a unifying contract. This is the path of least resistance but leaves the
"distributed constraint" problem intact and does not provide an inspectable,
diffable policy artefact.

### Option B: LLM-evaluated policy

Use the LLM itself to evaluate whether an action complies with intent.
This is explicitly rejected: OWASP LLM07:2025 and the intent-engineering
analysis both note that critical controls must not be delegated to the
LLM. [^1] [^7] Deterministic evaluation is a hard requirement.

## Open questions

- Should the authoritative contract live in `context/intent.md`,
  per-project `intent.yml`, per-job metadata, or all three with precedence
  rules? The proposed design supports all three, but the canonical
  location for workspace-level contracts needs a convention.
- Which current files are trusted by default — the existing identity and
  control-plane files only, or an explicit allowlist? The safer default is
  an explicit allowlist that initially contains only identity files.
- When a gate check fails, should some action classes downgrade to
  explicit approval without silently widening authority? The proposed
  `escalate` mode provides this, but the set of action classes that
  qualify for escalation versus hard denial needs further specification.
- How should the contract interact with skill trust levels? Trusted skills
  already have broader tool access; the contract should express this
  relationship rather than creating a parallel trust hierarchy.

## Recommendation

Adopt the intent contract as a first-class artefact with fail-closed
runtime gates. Start with a shadow mode for migration, enforce
narrowing-only scope composition, and treat all retrieved content as
untrusted unless explicitly promoted. Use the contract as the structured
input to an external policy engine (ADR 001) rather than embedding policy
logic in application code.

---

[^1]: Intent-engineering analysis. See
    `docs/What Axinite can learn from the video's approach to intent
    engineering.md`.

[^2]: Agent skills support. See `docs/agent-skills-support.md`, skill
    activation and tool attenuation sections.

[^3]: RFC 0006: Provenance-based, zero-knowledge intent plugins. See
    `docs/rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md`.

[^4]: Safety layer. See `SafetyLayer` in `src/safety/`.

[^5]: Workspace seeded structure. See `docs/chat-model.md`, identity files
    and workspace sections.

[^6]: Agent module, hook failure and routine execution. See
    `docs/jobs-and-routines.md`.

[^7]: OWASP Top 10 for LLM Applications 2025. LLM01:2025 (Prompt
    Injection), LLM06:2025 (Excessive Agency). See
    <https://genai.owasp.org/llm-top-10/>.

[^8]: Indirect prompt injection research. See OWASP LLM Prompt Injection
    Prevention Cheat Sheet,
    <https://cheatsheetseries.owasp.org/cheatsheets/LLM_Prompt_Injection_Prevention_Cheat_Sheet.html>.
