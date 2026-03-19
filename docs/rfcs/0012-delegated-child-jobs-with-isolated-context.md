# RFC 0012: Delegated child jobs with isolated context

## Preamble

- **RFC number:** 0012
- **Status:** Proposed
- **Created:** 2026-03-15

## Summary

Axinite should introduce a first-class child-job delegation primitive
that creates isolated context, explicit tool allowlists,
iteration/token/cost/time ceilings, and optional workspace isolation
such as a git worktree or sandbox copy. The primitive should return only
a distilled result plus an out-of-band evidence bundle to the parent
context.

Axinite already has most of the raw machinery: scheduler-managed jobs,
`Task::ToolExec` subtasks that return over a oneshot channel, routine
`lightweight` versus `full_job` execution with iteration caps and
restricted tools, and the Monty subprocess direction. [^1] However,
those pieces do not yet form a coherent, model-visible delegation
contract, and subtasks currently do not inherit the parent approval
context. [^1]

The Hermes Agent analysis points squarely at productizing isolation and
budgeting, not merely spawning more loops. [^2]

## Problem

Axinite's agent currently handles complex, multi-step tasks in a single
context window. As tasks grow in scope, this creates several problems:

- **Context pressure**: long-running tasks accumulate tool outputs,
  intermediate results, and reasoning traces that consume the context
  window, increasing the likelihood of compaction-triggered information
  loss and increasing inference cost. [^2]
- **Blast radius**: a tool error or model reasoning failure in one part
  of a complex task can corrupt the entire context, requiring the
  operator to restart from scratch.
- **Tool permission sprawl**: the agent sees all available tools for the
  entire task, even when only a subset is relevant to the current
  subtask. OWASP LLM06:2025 (Excessive Agency) recommends minimizing
  permissions granted to LLM applications. [^3]
- **No budget enforcement**: there is no mechanism to cap the cost,
  iterations, or time spent on a subtask independently of the parent.
- **No workspace isolation**: parallel work on the same repository
  risks clobbering changes. Hermes Agent explicitly addresses this via
  git worktree isolation. [^2]

### Current subtask limitations

The scheduler's `Task::ToolExec` variant supports subtasks that run
through the safety layer and approval checks, but with significant
limitations:

- Subtasks do not create durable job records.
- Subtasks return through a oneshot channel and are tracked separately
  from full jobs.
- Subtask execution does not yet propagate the parent job's
  `ApprovalContext`, noted as a TODO in the source. [^1]
- There is no model-visible tool for creating subtasks. Delegation is
  an internal mechanism, not a user-facing capability.

## Current state

### Execution families

Axinite distinguishes four execution families: [^1]

1. **In-process agent jobs**: scheduler-managed large language model
   (LLM) worker runs with full tool access, persistence, and approval
   gating.
2. **Sandbox jobs**: containerized execution via an orchestrator with
   separate worker runtime.
3. **Routines**: proactive rules that run either inline (`lightweight`
   with hard iteration cap and restricted tools) or delegated
   (`full_job` via the scheduler).
4. **Heartbeat**: a separate periodic runner with narrow scope.

### Relevant primitives

<!-- markdownlint-disable MD013 -->
| Primitive | Current capability | Gap for delegation |
| --- | --- | --- |
| `Task::Job` | Full LLM-driven job. Must go through `schedule()` and full persistence. | No isolation from parent context. No model-visible creation interface. |
| `Task::ToolExec` | Lightweight tool execution tied to a parent job. | No approval context inheritance. No budget enforcement. No result distillation. |
| `Task::Background` | Custom async handler. | No LLM loop. Framework extension point only. |
| Routine `lightweight` | Inline execution with 3–5 iteration cap, `Never`-approval tools only. | Not model-callable. No result return to parent context. |
| Routine `full_job` | Delegates to scheduler. Returns quickly. | No parent-child relationship in the scheduler. No result aggregation. |

_Table 1: Current primitives and delegation gaps._
<!-- markdownlint-enable MD013 -->

### Delegated endpoints

RFC 0006's "tokenized delegated authorized endpoint requests" provides
a security vocabulary directly relevant to safe delegation: opaque
capability tokens let the host resolve identities and inject
credentials while redacting the concrete endpoint from agent-visible
surfaces. [^4] The transport-authority vs guest-authority split
provides a tested mental model for what a delegated worker can see
and do.

## Goals and non-goals

- Goals:
  - Provide a model-callable `delegate_task` tool (or equivalent) that
    creates a child job with explicit isolation, budgets, and tool
    scoping.
  - Return only a distilled result to the parent context, with full
    evidence stored out-of-band.
  - Support optional workspace isolation (git worktree, sandbox copy,
    or filesystem copy-on-write).
  - Enforce iteration, token, cost, and time budgets at the child-job
    boundary.
  - Propagate or narrow the parent's intent contract and approval
    context.
  - Record delegation events in the execution ledger (RFC 0011).
- Non-goals:
  - Replace the existing scheduler or routine engine. The delegation
    primitive builds on them.
  - Support arbitrary nesting without limits. The intent contract's
    `max_delegation_depth` constrains recursion (RFC 0010).
  - Provide a general-purpose multi-agent orchestration framework.
    Delegation is one parent spawning one or more bounded child jobs,
    not a swarm.

## Proposed design

### 1. Delegation contract

A delegation contract is a structured document specifying:

<!-- markdownlint-disable MD013 -->
| Field | Type | Description |
| --- | --- | --- |
| `goal` | string | What the child job should accomplish. |
| `tool_allowlist` | list | Tools the child job may use. Subset of parent's allowed tools. |
| `tool_denylist` | list | Tools explicitly denied. |
| `max_iterations` | integer | Hard cap on reasoning iterations. |
| `max_tokens` | integer | Token budget for LLM calls. |
| `max_cost` | decimal | Cost ceiling in billing units. |
| `timeout` | duration | Wall-clock time limit. |
| `workspace_isolation` | enum | `none`, `worktree`, `sandbox`, `copy_on_write`. |
| `result_format` | enum | `summary_only`, `summary_with_refs`, `resumable_handle`. |
| `approval_inheritance` | enum | `inherit`, `narrow`, `fresh`. |

_Table 2: Delegation contract fields._
<!-- markdownlint-enable MD013 -->

### 2. Model-visible tool interface

The delegation primitive is exposed as a built-in tool:

<!-- markdownlint-disable-next-line MD013 -->
```json
{
  "name": "delegate_task",
  "description": "Spawn a child job with isolated context and budgets.",
  "parameters": {
    "goal": "string (required)",
    "tools": ["string"],
    "max_iterations": "integer (default: 10)",
    "timeout_seconds": "integer (default: 300)",
    "workspace_isolation": "string (default: none)",
    "result_format": "string (default: summary_only)"
  }
}
```

The tool returns a structured result containing a text summary and,
optionally, references to evidence artefacts stored in the workspace
or job metadata.

### 3. Execution path

1. The model calls `delegate_task` with a goal and optional parameters.
2. The runtime constructs a delegation contract by narrowing the
   parent's intent contract (RFC 0010). The child contract cannot
   widen any parent constraint.
3. If workspace isolation is requested, the runtime creates the
   isolation environment (git worktree, sandbox container, or
   copy-on-write directory).
4. The runtime dispatches a child job through the scheduler's existing
   `dispatch_job_with_context()` path, attaching the delegation
   contract and budget parameters.
5. The child job runs with its own context window, restricted tools,
   and budget enforcement.
6. On completion (or budget exhaustion), the child job produces:
   - A distilled text summary (always).
   - An evidence bundle stored in the workspace or job metadata
     (optional, based on `result_format`).
7. The runtime returns only the summary (and optionally evidence
   references) to the parent context.
8. A delegation ledger entry is recorded (RFC 0011).

### 4. Approval context handling

Three modes are supported:

- **`inherit`**: the child job inherits the parent's `ApprovalContext`
  directly. Tools that the parent has already approved remain approved.
- **`narrow`**: the child job starts with the parent's approvals but
  cannot escalate to tools that require higher approval levels. New
  approval requests from the child are routed to the parent's operator.
- **`fresh`**: the child job starts with no pre-approved tools. All
  approval-gated tools require fresh approval.

The default is `narrow`, which provides the least-privilege default
while avoiding excessive approval friction.

### 5. Workspace isolation

<!-- markdownlint-disable MD013 -->
| Mode | Mechanism | Use case |
| --- | --- | --- |
| `none` | Child job shares the parent's working directory. | Read-only analysis, summarization. |
| `worktree` | Creates a git worktree for the child job. Changes are isolated until explicitly merged. | Parallel code modifications. |
| `sandbox` | Dispatches to the sandbox/container job path. | Untrusted or risky operations. |
| `copy_on_write` | Creates a filesystem-level copy-on-write snapshot. | File-system operations without git. |

_Table 3: Workspace isolation modes._
<!-- markdownlint-enable MD013 -->

### 6. Budget enforcement

Budget enforcement is the child job's responsibility at the worker
level, but the scheduler monitors for runaway children:

- **Iteration cap**: the worker checks the iteration count before each
  reasoning step.
- **Token cap**: the worker tracks cumulative token usage and stops
  when the budget is exhausted.
- **Cost cap**: the worker tracks cumulative cost (using provider
  pricing metadata) and stops when the budget is exhausted.
- **Time cap**: the scheduler enforces a wall-clock timeout, killing
  the child job if it exceeds the limit.

When a budget is exhausted, the child job produces a partial result
with an explicit "budget exhausted" status.

## Requirements

### Functional requirements

- The `delegate_task` tool must be callable by the model during normal
  agentic reasoning.
- Child jobs must run with isolated context that does not pollute the
  parent conversation.
- Tool allowlists must be enforced at the child job's safety layer.
- Budget enforcement must be deterministic and non-bypassable.
- The parent must receive only a distilled result, not the full child
  context.
- Delegation events must produce execution ledger entries.

### Technical requirements

- Child jobs must use the existing scheduler/worker infrastructure.
- Workspace isolation must support at least the `none` and `worktree`
  modes in the initial implementation.
- The delegation contract must be serializable and persistable with
  job metadata.
- Child-job results must be returned to the parent via the existing
  oneshot/channel mechanism or an equivalent.

## Compatibility and migration

The delegation primitive is additive. Existing jobs, routines, and
subtasks continue to function unchanged. The `delegate_task` tool is
registered alongside existing built-in tools and is subject to the
same skill-based tool attenuation.

## Alternatives considered

### Option A: Extend `Task::ToolExec` with budgets

Add budget parameters to the existing subtask mechanism. This avoids
creating a new job type but does not provide context isolation,
workspace isolation, or result distillation.

### Option B: Use routines as delegation targets

Create a routine for each delegated task. Routines already support
`lightweight` (bounded) and `full_job` (scheduler-managed) modes. [^1]
However, routines are persistent, user-owned records with triggers and
notifications — too heavy for ephemeral delegation.

### Option C: External multi-agent framework

Delegate to an external orchestration system. This adds operational
complexity and latency, and does not leverage Axinite's existing
scheduler, safety layer, or approval infrastructure.

## Open questions

- Should child jobs inherit the parent approval context, derive a
  stricter one, or require fresh approval boundaries? The proposed
  `narrow` default is the recommended starting point, but the set of
  tools that qualify for inheritance needs further specification.
- When is workspace isolation mandatory rather than optional? Code
  modification tasks probably require at least `worktree` isolation,
  but read-only analysis tasks do not. Should the runtime infer the
  isolation mode from the tool allowlist?
- Should the parent receive only a summary, a summary plus evidence
  references, or a resumable child-job handle? The proposed
  `result_format` enum supports all three, but the "resumable handle"
  mode adds significant complexity.
- How should child-job failures propagate to the parent? Options
  include: return an error summary (default), retry with a different
  strategy, or escalate to the operator.

## Recommendation

Implement the `delegate_task` tool as a first-class built-in that
dispatches through the existing scheduler, with narrowing-only contract
inheritance, explicit tool allowlists, and mandatory budget
enforcement. Start with `none` and `worktree` workspace isolation
modes, `summary_only` result format, and `narrow` approval inheritance.
Extend to sandbox isolation, evidence bundles, and resumable handles as
the delegation model matures.

---

[^1]: Jobs and routines architecture, scheduler, `Task` model, and
    subtask limitations. See `docs/jobs-and-routines.md`.

[^2]: Hermes Agent analysis, sub-agent patterns. See
    `docs/Axinite lessons from Hermes Agent on provider resilience
    and sub-agents.md`.

[^3]: OWASP Top 10 for LLM Applications 2025. LLM06:2025 (Excessive
    Agency). See <https://genai.owasp.org/llm-top-10/>.

[^4]: RFC 0006: Provenance-based, zero-knowledge intent plugins.
    Tokenized delegated authorized endpoint requests. See
    `docs/rfcs/0006-provenance-based-zero-knowledge-intent-plugins.md`
    and
    `docs/rfcs/0004-tokenized-delegated-authorized-endpoint-requests.md`.
