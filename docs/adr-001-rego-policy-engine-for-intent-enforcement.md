<!-- markdownlint-disable-next-line MD013 -->
# Architectural decision record (ADR) 001: OPA Rego as the policy engine for intent enforcement

## Status

Proposed.

## Date

2026-03-15.

## Context and problem statement

Axinite's intent contract (RFC 0010) requires a deterministic policy
evaluation engine that can accept structured inputs — intent contract,
tool schema, provenance labels, approval context, workspace scope, and
sink type — and produce auditable, machine-readable decisions. The
current safety mechanisms use a combination of regex-based rules,
hardcoded approval checks, and allowlist validation. [^1] [^2] These are
effective for their current scope but do not scale to the richer
constraint vocabulary that intent contracts introduce.

RFC 0006 already recommends "Rego for enforcement, optionally Starlark
for authoring convenience" and notes that OPA supports compiling Rego
policies to WebAssembly for embedding. [^3] This ADR records that
decision formally and specifies the integration shape.

The key requirement is that policy evaluation must be:

- **Deterministic**: the same inputs must always produce the same
  decision.
- **Auditable**: every decision must produce a machine-readable reason
  that can be recorded in the execution ledger (RFC 0011).
- **Fast**: evaluation must run synchronously on the tool-execution
  critical path without perceptible latency.
- **Embeddable**: the policy engine must run in-process in a Rust binary
  without requiring a sidecar or network call.
- **Deny-by-default**: an absent or malformed policy must deny rather
  than permit.

## Decision drivers

- Axinite already uses WebAssembly (WASM) (via `wasmtime`) as a core
  isolation boundary for tool execution. [^1] Adding WASM-compiled policy
  evaluation reuses the same runtime dependency and operational model.
- RFC 0006 explicitly recommends Rego over Starlark for enforcement,
  noting that Rego was purpose-built for expressing policy over
  structured inputs. [^3]
- OWASP LLM06:2025 (Excessive Agency) recommends that security controls
  be enforced independently from the language model (LLM) in a
  deterministic, auditable manner. [^4]
- National Institute of Standards and Technology (NIST) AI Risk
  Management Framework (RMF) 1.0 positions trustworthy AI as requiring
  governance, measurement, and operationalization across the full
  lifecycle, not solely modelling-time controls. [^5]

## Options considered

### Option A: OPA Rego compiled to WebAssembly

Open Policy Agent (OPA) Rego is a purpose-built policy language with
deny-by-default semantics. OPA compiles Rego policies to WebAssembly
modules using `opa build -t wasm`, producing bundles that expose an
`opa_eval` entry point for one-shot policy evaluation. [^6] The WASM
application binary interface (ABI) (version 1.2+) accepts JSON-
serialized input and data, and returns JSON-serialized decision
results. [^6]

Rust integration is available through multiple paths:

- **`opa-wasm`** (Matrix.org): a Rust software development kit (SDK)
  that evaluates OPA WASM bundles using `wasmtime`. Actively maintained,
  supports `wasmtime` 22–40, Apache-2.0 licensed. [^7]
- **Regorus** (Microsoft): a native Rust Rego interpreter (~85–90%
  coverage of OPA v1.2.0, ~10x faster than OPA Go on benchmarks),
  MIT-licensed. Does not require WASM compilation. [^8]
- **Direct `wasmtime` integration**: load the OPA-compiled WASM module
  directly via Axinite's existing `wasmtime` infrastructure.

### Option B: Starlark

Starlark is deterministic and hermetic by design, and is well-suited as
a configuration language. However, it is a general-purpose language that
tends to produce less auditable policy-as-code in practice. RFC 0006
explicitly positions Starlark as optional authoring sugar, not as the
enforcement engine. [^3]

### Option C: Cedar (Amazon)

Cedar is a purpose-built authorization policy language with formal
verification properties. It has strong semantics for role-based access
control (RBAC) and attribute-based access control (ABAC) but is less
flexible for the kinds of structured-input policy evaluation that
intent contracts require (e.g. provenance-label checks, sink-promotion
constraints). Cedar's Rust SDK is well-maintained but adds a new
dependency with a different operational model from Axinite's existing
WASM infrastructure. [^9]

### Option D: Custom Rust DSL

Build a bespoke policy language in Rust. This avoids external
dependencies but requires maintaining a policy language, parser, and
evaluator. The ongoing maintenance cost is disproportionate to the
benefit when mature alternatives exist.

<!-- markdownlint-disable MD013 -->
| Factor | OPA Rego (WASM) | Regorus (native) | Starlark | Cedar | Custom DSL |
| --- | --- | --- | --- | --- | --- |
| Purpose-built for policy | Yes | Yes (Rego) | No | Yes | No |
| Deny-by-default semantics | Yes | Yes | Manual | Yes | Manual |
| WASM compilation | Native | Compiles to WASM | No | No | N/A |
| Rust ecosystem support | `opa-wasm`, Regorus | Native Rust | `starlark-rust` | `cedar-policy` | N/A |
| Auditable decision output | Built-in | Built-in | Manual | Built-in | Manual |
| Aligns with existing WASM infra | Yes | Partially | No | No | No |
| RFC 0006 recommendation | Yes | Yes | Authoring only | Not considered | Not considered |

_Table 1: Policy engine comparison._
<!-- markdownlint-enable MD013 -->

## Decision outcome / proposed direction

**Use OPA Rego as the normative policy language.** The recommended
integration path has two viable branches:

1. **Primary**: use Regorus as a native Rust Rego interpreter for
   in-process evaluation. This avoids the WASM compilation step, is
   faster (~10x over OPA Go), and provides a simpler integration surface
   for Axinite's Rust codebase. [^8]
2. **Alternative**: use `opa-wasm` or direct `wasmtime` integration to
   evaluate OPA-compiled WASM bundles. This is appropriate if the full
   OPA built-in function set is required or if policy distribution uses
   OPA's existing bundle format. [^7]

Both paths use the same Rego policy source files, so the choice of
evaluation engine does not affect policy authorship.

Rego is the source of truth for enforcement logic. Any Starlark or JSON
authoring layer is optional sugar, not a second policy system.

## Goals and non-goals

- Goals:
  - Evaluate intent contracts against structured inputs at every gate
    point defined in RFC 0010.
  - Produce machine-readable decision reasons that flow into the
    execution ledger (RFC 0011).
  - Support policy versioning, bundling, and distribution alongside
    capability schemas.
  - Deny by default when a policy is absent, malformed, or fails to
    evaluate.
- Non-goals:
  - Replace the existing `SafetyLayer` mechanisms. Rego policies
    augment and formalize existing checks; they do not replace
    runtime sanitization or secret-leak detection.
  - Implement a full OPA server or bundle API. Evaluation is
    in-process only.
  - Define the policy content. This ADR specifies the engine; the
    policies themselves are authored per-deployment.

## Integration design

### Structured input schema

Every policy evaluation receives a JSON input document containing:

Screen-reader: example policy evaluation input payload showing contract,
action, provenance, approval, workspace, and sink fields.

```json
{
  "contract": { "scope": "job", "allowed_tool_families": [...], ... },
  "action": { "type": "tool_call", "tool": "memory_write", "params": {...} },
  "provenance": { "trust_label": "untrusted", "source": "workspace_recall" },
  "approval": { "context": "job_123", "granted": ["memory_read"] },
  "workspace": { "id": "ws_abc", "thread_id": "thr_456" },
  "sink": { "type": "workspace_document", "path": "projects/x/notes.md" }
}
```

### Decision output schema

Every evaluation produces a decision record:

Screen-reader: example policy evaluation decision output payload showing
allowed, reason, policy_version, contract_scope, and timestamp fields.

```json
{
  "allowed": false,
  "reason": "tool 'memory_write' not in allowed_tool_families",
  "policy_version": "0010-v1",
  "contract_scope": "job",
  "timestamp": "2026-03-15T12:00:00Z"
}
```

This record is appended to the execution ledger (RFC 0011).

### Policy-evaluation failure

If the Rego evaluator fails (crash, timeout, malformed policy), the
gate denies the action. There is no fallback to a permissive default.
The failure is logged with full diagnostic context and emitted over
Server-Sent Events (SSE).

## Known risks and limitations

- **Rego learning curve**: operators authoring custom policies need
  familiarity with Rego syntax. Mitigated by providing well-documented
  default policies and optional Starlark authoring sugar.
- **Regorus coverage gaps**: Regorus covers ~85–90% of OPA v1.2.0
  built-ins, with gaps in JSON Web Token (JWT) operations, network
  Classless Inter-Domain Routing (CIDR) functions, and GraphQL. [^8]
  These gaps are unlikely to affect intent-contract evaluation, which
  operates on JSON-structured inputs.
- **Performance under complex policies**: Rego evaluation is typically
  sub-millisecond for simple policies, but complex policies with large
  data sets may require benchmarking. The chaos-test infrastructure
  should include policy-evaluation latency tests.

## Outstanding decisions

- Whether to use Regorus (native Rust) or `opa-wasm` (WASM bundle
  evaluation) as the primary runtime. Regorus is recommended for
  simplicity and performance; `opa-wasm` is the fallback if full OPA
  compatibility is required.
- Whether Starlark authoring sugar should be provided in the initial
  implementation or deferred.
- The canonical location for policy files within the workspace
  structure (e.g. `policies/` directory, bundled with skill
  definitions, or embedded in the intent contract).

---

[^1]: WASM capability model and tool execution. See
    `src/tools/wasm/wrapper.rs` and `src/tools/wasm/allowlist.rs`.

[^2]: Safety layer. See `SafetyLayer` in `src/safety/`.

[^3]: RFC 0006: Provenance-based, zero-knowledge intent plugins. See
    `docs/rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md`.

[^4]: OWASP Top 10 for LLM Applications 2025. LLM06:2025 (Excessive
    Agency). See <https://genai.owasp.org/llm-top-10/>.

[^5]: NIST AI Risk Management Framework 1.0 and Generative AI Profile
    (AI 600-1). See <https://www.nist.gov/itl/ai-risk-management-framework>
    and <https://nvlpubs.nist.gov/nistpubs/ai/NIST.AI.600-1.pdf>.

[^6]: OPA WebAssembly documentation. See
    <https://www.openpolicyagent.org/docs/wasm>.

[^7]: `opa-wasm` Rust SDK (Matrix.org). See
    <https://github.com/matrix-org/rust-opa-wasm>.

[^8]: Regorus: native Rust Rego interpreter (Microsoft). See
    <https://github.com/microsoft/regorus>.

[^9]: Cedar policy language (Amazon). See
    <https://www.cedarpolicy.com/>.
