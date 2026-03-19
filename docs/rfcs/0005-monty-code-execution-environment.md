# RFC 0005: Monty-based Python code execution environment

## Preamble

- RFC number: 0005
- Status: Proposed
- Date: 2026-03-11
- Target: IronClaw tool system and worker runtime
- Authors: Codex draft for review

## Summary

IronClaw should integrate [Monty](https://github.com/pydantic/monty) as a
capability-brokered Python code execution environment for agent-written code and
pre-written scripts. The initial product shape should be a codemode runner, not
a classic line-oriented REPL.

The proposed surface is three operations:

1. `save_script(name, code, allowed_tools, entrypoint="main")`
2. `run_script(name, params, allowed_tools, state=None)`
3. `exec_code(code, allowed_tools, params=None, state=None)`

Saved scripts give IronClaw a durable, reviewable automation format. `exec_code`
covers ephemeral scratch execution. Both run through the same host-mediated tool
broker, use a JSON-only ABI across the guest/host boundary, and return updated
`state` explicitly rather than depending on hidden interpreter globals.

Monty is a strong fit because it is designed to run code written by agents,
supports host-controlled external functions, type checking, resource limits, and
pause/resume snapshots at external calls. It is a weaker fit for a persistent
live REPL because the upstream REPL surface is still moving and current open
issues show both missing ergonomics and panic-related stability risks.[^1][^2][^3]

## Problem

IronClaw already has strong capability mediation for tools, memory, sandboxing,
and approvals, but it does not have a first-class environment for programmatic
tool use in Python.

That gap shows up in three places:

1. Traditional language model (LLM) tool calling is awkward for loops,
   filtering, retries, and
   conditional multi-step tool use.
2. Pre-written automations are currently forced into either free-form prompt
   text or bespoke Rust/WebAssembly (WASM) tools.
3. A full containerized Python runtime would duplicate existing safety layers
   while adding significant startup cost, image management, and operational
   complexity.

The desired scope is narrower than "general Python support". The intended
execution surface is a small Python environment that:

- runs agent-authored or user-authored scripts,
- can call a selected subset of IronClaw tools,
- cannot access the host directly unless the host exposes that access,
- can branch and loop over JSON-shaped data,
- is easy to inspect, replay, and audit.

## Goals

- Add a Python execution environment aimed at tool-calling automation, not
  arbitrary package execution.
- Support saved scripts that can be reviewed, named, rerun, and invoked from
  higher-level workflows later.
- Keep all filesystem, network, secret, and environment access behind IronClaw
  tool mediation.
- Reuse existing approval and tool attenuation concepts rather than inventing a
  separate policy system.
- Keep cross-boundary data boring and serializable.
- Preserve a credible safety story even if Monty itself panics.

## Non-Goals

- Full CPython compatibility.
- Third-party Python package support.
- A polished classic REPL in the first iteration.
- Hidden mutable interpreter state shared across unrelated executions.
- A new general-purpose sandbox stack parallel to IronClaw's existing Docker
  worker model.

## Why Monty

Monty matches the desired execution model unusually well:

- It is explicitly built to run code written by agents.[^1]
- Filesystem, environment variables, and network access are mediated through
  external functions the host chooses to expose.[^1]
- It supports host callbacks and iterative pause/resume at external calls via
  `start()` and `resume()`.[^1]
- It supports serialization of compiled programs and in-flight snapshots.[^1]
- It supports resource limits covering memory, allocations, stack depth, and
  execution time.[^1]
- Its examples already use async host functions and `list[dict[str, Any]]`
  values, which is the right shape for IronClaw tool results.[^1]

This is a better fit than introducing Bun, Deno, or a full containerized Python
stack just to gain structured tool calls, loops, and control flow.

## Upstream Constraints

Monty is promising, but the integration should be shaped around its current
reality rather than its best-case future.

### 1. Upstream still calls Monty experimental

The README currently describes Monty as experimental and not ready for prime
time.[^1]

That is acceptable for a narrow, brokered integration. It is not enough to
justify optimistic availability claims.

### 2. REPL support is not the stable centre of gravity

The most important current upstream evidence points away from a "persistent live
REPL first" design:

- Issue [#190](https://github.com/pydantic/monty/issues/190) requests
  suspendable `feed` support plus dynamic per-call external functions for REPL
  mode because the Python binding currently exposes synchronous `feed()` rather
  than the same pause/resume loop available in one-shot execution.[^2]
- Issue [#239](https://github.com/pydantic/monty/issues/239) requests external
  function support for `MontyRepl`; a maintainer comment says it "should be
  fixed by #235", but the issue remained open on 2026-03-11 and the next release
  status was not resolved in the visible thread.[^3]

That means a classic REPL may become viable, but it is not the safest foundation
for IronClaw phase one.

### 3. Availability risks are real

Two open issues matter directly for host reliability:

- Issue [#208](https://github.com/pydantic/monty/issues/208) reports that a VM
  panic can crash the whole server; the maintainer response does not claim a
  host-side workaround beyond fixing the panic upstream.[^4]
- Issue [#240](https://github.com/pydantic/monty/issues/240) reports another
  Rust panic path around future snapshots, with a proposed fix under
  PR [#251](https://github.com/pydantic/monty/pull/251).[^5]

These issues do not kill the proposal. They do rule out an in-process-only
integration as the default safety posture.

## Existing IronClaw Primitives to Reuse

IronClaw already has most of the host-side machinery this integration needs.

### Tool attenuation and schema export

`src/tools/registry.rs` already exposes:

- `retain_only(&self, names: &[&str])`
- `tool_definitions(&self)`
- `tool_definitions_for(&self, names: &[&str])`

That means IronClaw can already derive a per-run allowlist and produce a
selected tool definition set without inventing a second registry.

### Worker and sandbox boundaries

IronClaw already supports container-domain execution in worker processes and
Docker-backed workers. A Monty runner should live inside the current execution
boundary:

- inside the worker/container when Docker sandboxing is enabled,
- in a local helper subprocess when sandboxing is disabled.

This keeps the Monty child on the same side of policy decisions as the code that
would actually execute tools.

### Workspace-backed persistence

IronClaw's workspace system already supports arbitrary path-based documents and
routine state storage under namespaced paths. Saved scripts should be persisted
the same way instead of adding a separate storage system.

## Proposal

### Product shape

Phase one should ship a codemode runner with two execution modes:

1. Saved script execution for reusable automations.
2. Ephemeral code execution for one-off scratch work.

A classic persistent REPL should be explicitly deferred until upstream REPL
support and crash behaviour are mature enough to justify the surface area.

### Public operations

#### `save_script`

Screen-reader description: public operation signature for saving execution
scripts.

```text
save_script(
  name,
  code,
  allowed_tools,
  entrypoint="main",
  description=None,
)
```

Behaviour:

- Validates the script name.
- Stores the source and manifest in the workspace.
- Records an allowlist ceiling for future runs.
- Requires an explicit `allowed_tools` allowlist. Callers that want no tool
  access must pass an empty list so the API stays fail-closed.
- Optionally compiles and type-checks up front so errors are caught at save time.

Recommended workspace layout:

Screen-reader description: recommended workspace file layout for saved
scripts and optional persistent state.

```text
scripts/<safe_name>/script.py
scripts/<safe_name>/manifest.json
scripts/<safe_name>/README.md      # optional, human-authored notes
scripts/<safe_name>/state.json     # optional, only if caller wants persisted state
```

Manifest fields:

- `name`
- `entrypoint`
- `allowed_tools`
- `description`
- `created_at`
- `updated_at`
- `sha256`
- `monty_version`

#### `run_script`

Screen-reader description: public operation signature for running a saved
script with parameters and limits.

```text
run_script(
  name,
  params,
  allowed_tools,
  state=None,
  limits=None,
)
```

Behaviour:

- Loads the saved script and manifest.
- Computes the effective allowlist as the intersection of the saved allowlist
  and the explicit per-run `allowed_tools` request.
- Runs the script with named `params` and `state`.
- Returns structured output plus the updated `state`.

`allowed_tools` is mandatory at call time. Pass an empty list to request no
tool access. The effective allowlist must never widen the saved allowlist.

#### `exec_code`

Screen-reader description: public operation signature for ad hoc code
execution with explicit tool limits.

```text
exec_code(
  code,
  allowed_tools,
  params=None,
  state=None,
  entrypoint="main",
  limits=None,
)
```

Behaviour:

- Compiles and runs ad hoc code without persisting the source by default.
- Requires an explicit `allowed_tools` allowlist. Callers that want no tool
  access must pass an empty list so scratch execution also stays fail-closed.
- Uses the same tool broker and ABI as `run_script`.
- Returns structured output plus the updated `state`.

This is intentionally scratch-oriented. It should not become the default place
to hide durable automation logic.

## Script Contract

The guest contract should stay narrow and unsurprising.

### Entrypoint

The design should standardize on a simple async entrypoint:

Screen-reader description: canonical async Python entrypoint receiving
explicit params and state.

```python
from typing import Any

async def main(
    params: dict[str, Any],
    state: dict[str, Any],
) -> dict[str, Any]:
    ...

await main(params, state)
```

This gives the script explicit inputs and explicit persisted state. It avoids a
large implicit global namespace and maps directly onto Monty's named input model.

### Return value

The script result should be a JSON-like object with this conventional shape:

Screen-reader description: conventional JSON-like result object returned by a
script.

```python
{
    "result": ...,
    "state": state,
    "artifacts": [...],  # optional
    "logs": [...],       # optional
}
```

IronClaw should require that `state` be a dictionary when returned. If the
script omits `state`, the runner should treat that as a contract error.

## Guest/Host ABI

The Monty boundary should only allow JSON-shaped values:

- `null`
- booleans
- numbers
- strings
- lists
- objects

Explicitly out of scope for phase one:

- file handles
- streams
- raw bytes
- host object references
- callbacks from the guest into arbitrary host objects

If bytes are ever needed, they should be encoded into strings at the boundary.

This ABI choice makes:

- approval prompts simpler,
- serialization predictable,
- replay and audit sane,
- snapshot persistence tractable,
- tool stub generation far less brittle.

## Tool Broker Design

The key architectural idea is a session-local tool broker between Monty and the
real IronClaw tool registry.

### Allowlist derivation

For each run:

1. Start from the available IronClaw `ToolRegistry`.
2. Intersect it with the script's allowed tools and any per-run override.
3. Export definitions only for that selected subset.
4. Generate Monty stubs and the external function table from that subset.

The guest never sees the full registry.

### Stub generation

The broker should generate Python stubs from IronClaw `ToolDefinition` objects.
The first version should optimize for reliability over perfect typing.

Recommended first-pass stub shape:

Screen-reader description: example generated Python tool stubs exported to
guest code.

```python
from typing import Any

async def memory_search(*, query: str, limit: int | None = None) -> Any: ...
async def memory_write(*, path: str, content: Any) -> Any: ...
```

Notes:

- Required top-level JSON schema properties become required keyword-only
  parameters.
- Optional properties become optional keyword-only parameters.
- Nested objects and complex return types can start as `Any`.
- Tool descriptions should be emitted into docstrings to keep scripts legible to
  both humans and models.

This is enough to get useful type checking without pretending the schema mapping
is richer than it really is.

### Execution loop

The host-side loop should look like this:

1. Build the selected tool stub module and named inputs.
2. Start Monty execution.
3. If Monty yields a function snapshot, inspect `function_name` and arguments.
4. Re-check that the tool is still allowed.
5. Run the real IronClaw tool through the normal policy and approval path.
6. Normalize the tool result to the JSON ABI.
7. Resume Monty with that result.
8. Continue until completion or error.

This keeps every side effect inside IronClaw's existing mediation model rather
than giving the guest its own direct access path.

## Isolation Model

Monty should run in a dedicated helper subprocess by default.

### Why a subprocess

This is the right compromise between performance and blast-radius control.

- It is dramatically lighter than introducing a new full container stack.
- It prevents a Monty panic from taking down the parent process.
- It preserves the parent as the owner of approvals, policy, credentials, and
  tool execution.
- It allows the parent to enforce kill-on-timeout and kill-on-memory-pressure
  behaviour independently of Monty's internal accounting.

### Placement

The helper process should run inside the current execution boundary:

- If IronClaw is already inside a Docker worker, spawn the Monty helper inside
  that worker.
- If IronClaw is not sandboxed, spawn the helper as a child of the main local
  process.

This avoids a split-brain model where policy runs in one place and execution in
another.

### Parent/child protocol

The subprocess protocol should be boring:

- `compile`
- `start_run`
- `resume_run`
- `cancel_run`
- `dump_snapshot`
- `load_snapshot`

Messages should be JSON only. The child should never execute tools directly. It
should only request host callbacks.

### Error and timeout semantics

The helper protocol should classify failures explicitly so every worker reports
them the same way.

Recommended child-to-parent terminal outcomes:

- `completed`: the script returned a valid JSON-ABI result.
- `script_error`: guest code raised an exception or returned a contract-invalid
  value such as a non-object top-level result or missing `state`.
- `tool_error`: a host callback failed because the underlying IronClaw tool was
  denied, timed out, or returned a normal tool error.
- `tool_contract_error`: the host callback result could not be converted to the
  JSON ABI, or the child resumed with malformed callback data.
- `resource_limit_exceeded`: Monty reported an internal execution or memory
  limit failure.
- `execution_timeout`: the parent-side deadline expired and the helper was
  terminated.
- `runtime_crash`: the helper exited unexpectedly, panicked, or emitted
  malformed protocol frames.

Handling rules:

1. Parent-enforced wall-clock deadlines should be authoritative, even if Monty
   also has its own execution-time limit.
2. A helper panic, signal exit, or invalid JSON message should be surfaced to
   the caller as `runtime_crash`, with the run marked failed and the child
   discarded.
3. A tool result that cannot be normalized to the JSON ABI should fail the run
   as `tool_contract_error`; the parent should not attempt a best-effort resume.
4. Child-side guest exceptions should be returned as `script_error`, with a
   redacted traceback summary suitable for logs and operator inspection.
5. Tool denials, approval rejections, and ordinary tool execution failures
   should remain distinct from guest exceptions and be returned as
   `tool_error`.

Logging rules:

- Every run should emit a `run_id`, script identity, worker identity, terminal
  status, and elapsed time.
- `runtime_crash` and `execution_timeout` events should be logged at error
  level.
- `script_error`, `tool_error`, and `tool_contract_error` should include the
  failing tool name or guest frame summary when available, but never raw secret
  values or unredacted host-only data.

## Safety Model

The safety claim should be precise, not theatrical.

This integration can credibly claim:

- guest code has no direct filesystem, environment, or network access unless
  IronClaw exposes host functions for those capabilities,[^1]
- guest code can call only the external functions published for that run,[^1]
- every effectful operation still passes through IronClaw's host-side approval
  and policy path,
- execution is bounded by Monty limits plus parent-process kill switches.[^1]

The RFC should not claim:

- that Monty is production-hardened in the general case,
- that in-process execution is safe against host availability loss,
- that REPL-mode capabilities are already stable enough to be the primary user
  surface.

## State Model

State should be explicit.

### Per-run state

Every execution receives:

- `params`
- `state`

Every successful execution returns updated `state`.

This is the supported persistence mechanism across invocations.

### Saved state

If the caller wants durable state for a saved script, the runner may offer an
opt-in helper that reads and writes `scripts/<safe_name>/state.json` in the
workspace. That should be explicit and visible, not magic.

### Avoided persistence models

This design intentionally avoids:

- hidden interpreter globals,
- ambient variables that survive unrelated runs,
- a long-lived shared REPL session as the primary persistence model.

## Snapshots and Resume

Monty's snapshot support is useful, but phase one should use it conservatively.

### In scope now

- Internal pause/resume around host function calls.
- Optional compiled-program caching.
- Optional in-memory snapshot retention during a single parent-managed run.

### Deferred

- Durable cross-turn suspended runs stored in the database.
- User-visible resume tokens.
- Arbitrary REPL checkpointing.

This keeps the first implementation focused on the thing Monty already does well:
codemode-style host callback execution.

## Example

Screen-reader description: example Monty script that reads memory, filters
results, writes selected rows, and returns updated state.

```python
from typing import Any

async def main(
    params: dict[str, Any],
    state: dict[str, Any],
) -> dict[str, Any]:
    rows = await memory_search(
        query=params["query"],
        limit=params.get("limit", 10),
    )

    chosen = [
        row for row in rows
        if row.get("score", 0) >= params.get("min_score", 0.8)
    ]

    if chosen:
        await memory_write(
            path=params["out_path"],
            content={"rows": chosen},
        )

    state["last_count"] = len(chosen)
    return {"result": {"rows": chosen}, "state": state}

await main(params, state)
```

This is exactly the style of task where normal one-shot tool calling becomes
clumsy and a tiny code runner becomes useful.

## Alternatives Considered

### 1. Persistent REPL first

Rejected for phase one.

Reason:

- upstream REPL support is still shifting,[^2][^3]
- persistent hidden state is harder to reason about,
- approvals and snapshots become more complex,
- it encourages "keep poking at the session" behaviour instead of explicit,
  reviewable scripts.

### 2. Full CPython in Docker or a sandbox service

Rejected for this use case.

Reason:

- much higher startup and operational cost,
- larger blast radius,
- more moving parts than this feature needs for structured tool calling,
- weaker fit for explicit capability brokerage than Monty's external function
  model.[^1]

IronClaw already uses Docker workers where needed. That does not mean every
structured code execution feature should itself require a full Python container
runtime.

### 3. Bun or Deno codemode

Not preferred for the Python-first automation surface described here.

Reason:

- the request is specifically for Python,
- Monty's external function model is purpose-built for this niche,
- JavaScript runtimes solve a broader problem but do not obviously improve the
  capability-broker story for this feature.

### 4. In-process Monty

Rejected as the default.

Reason:

- open panic issues currently create avoidable availability risk.[^4][^5]

It may still be acceptable later behind a developer-only fast path once Monty
stability is better understood.

## Rollout Plan

### Phase 1: Ephemeral codemode runner

- Add a helper subprocess wrapper around Monty.
- Add `exec_code`.
- Generate stubs from an allowlisted subset of `ToolRegistry`.
- Normalize tool inputs and results to the JSON ABI.
- Gate every host callback through existing approval and policy checks.

### Phase 2: Saved scripts

- Add `save_script` and `run_script`.
- Store script source and manifest in the workspace under `scripts/`.
- Add opt-in persistent `state.json`.
- Add audit logging and version metadata for script runs.

### Phase 3: Workflow integration

- Allow routines and higher-level jobs to invoke saved scripts.
- Add curated built-in scripts for common IronClaw automations.
- Add UI/CLI affordances for reviewing, listing, and rerunning scripts.

### Phase 4: Re-evaluate REPL mode

Only after upstream REPL support and crash behaviour have stabilized:

- assess whether `MontyRepl` deserves a product surface,
- decide whether dynamic tool rebinding is mature enough,
- decide whether persistent live sessions are worth the operational complexity.

## Open Questions

1. Should saved scripts live only in the per-user workspace, or should there
   also be an admin-controlled global script catalog?
2. Should `save_script` type-check and compile eagerly, or should compile happen
   only at first run?
3. How much schema fidelity is worth encoding into generated Python stubs before
   nested types become noise?
4. Should `run_script` be exposed only as a tool first, or also as a CLI/API
   primitive in the first delivery?
5. Should durable suspended-run snapshots be a first-class database concept, or
   should that wait until a real use case appears?

## Recommendation

Adopt Monty for codemode-style Python execution in IronClaw, but do it in the
least glamorous way that is technically honest:

- codemode runner first,
- saved scripts plus ephemeral execution,
- explicit `params` and `state`,
- per-run attenuated tool exposure,
- JSON-only ABI,
- helper subprocess for fault isolation,
- no classic persistent REPL until upstream REPL support is genuinely ready.

That gives IronClaw the useful part of programmatic tool calling without
importing the complexity and fragility of a broader Python runtime story.

## References

[^1]: [Monty README](https://github.com/pydantic/monty/blob/main/README.md)
[^2]: [Support suspendable feed with dynamic external functions for REPL mode](https://github.com/pydantic/monty/issues/190)
[^3]: [Feature request: External function support for MontyRepl](https://github.com/pydantic/monty/issues/239)
[^4]: [VM Panic crashes whole server](https://github.com/pydantic/monty/issues/208)
[^5]: [FutureSnapshot triggers Rust Panic, PR Available](https://github.com/pydantic/monty/issues/240)
