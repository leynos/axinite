# RFC 0001: Expose MCP tool definitions to hosted LLMs

## Preamble

- **RFC number:** 0001
- **Status:** Proposed
- **Created:** 2026-03-11
- **Implementation status:** Roadmap items `1.1.1` and `1.1.2` are implemented
  in this branch through the shared `src/worker/api/` transport types, the
  worker catalog-fetch startup path, the orchestrator generic remote-tool
  execution endpoint, and the canonical `ToolRegistry`-owned hosted-visible
  filter for active MCP tools. Later roadmap items still own reasoning-context
  merge, broader schema parity checks, and end-to-end coverage.

## Summary

Before roadmap item `1.1.1`, hosted workers exposed only their local tool
registry plus a small set of extension-management proxy tools. That meant the
hosted large language model (LLM) did not see the real `ToolDefinition`s for
user-configured Model Context Protocol (MCP) tools, even when those tools were
already installed, activated, and available in the main IronClaw process.

The result is predictable: the model either cannot discover those tools at all,
or it sees only high-level extension-management tools rather than the real call
signatures, descriptions, and JSON Schemas. Tool selection quality degrades,
and tool calls become malformed because the model is reasoning from the wrong
interface.

This RFC proposes a narrow architectural correction:

1. Keep the orchestrator-side `ToolRegistry` as the canonical source of truth
   for active MCP tool definitions.
2. Expose a hosted-visible orchestrator tool catalogue to the worker.
3. Have the worker advertise those real tool definitions to the LLM unchanged.
4. Proxy execution of orchestrator-owned tools, including MCP tools, back
   through the orchestrator.

The LLM-facing interface remains the existing `ToolDefinition` shape:

```json
{
  "name": "github_search_repositories",
  "description": "Search repositories visible to the configured GitHub MCP server.",
  "parameters": {
    "type": "object",
    "properties": {
      "query": {
        "type": "string",
        "description": "Repository search query"
      }
    },
    "required": ["query"]
  }
}
```

The main change is not the schema format. It is which component supplies it.

## Problem

### Current hosted path

Today, the hosted worker builds its own local registry and advertises that
registry to the LLM:

- container-domain tools are registered inside the worker
- safe extension-management proxy tools are registered inside the worker
- the worker sends `self.tools.tool_definitions()` to the proxied LLM request

This is sufficient for shell, file, and worker-local meta tools. It is not
sufficient for user-configured third-party MCP tools, because those tools are
owned by the orchestrator-side extension system rather than the container.

### Current MCP path outside hosted mode

Outside hosted mode, IronClaw already has the right information:

- MCP activation creates real `Tool` implementations from the server's tool list
- those wrappers preserve the MCP tool `description`
- those wrappers preserve the MCP tool `input_schema`
- the active tool implementations are registered into the main `ToolRegistry`

In other words, IronClaw already has a canonical, correct representation of the
tool signatures and documentation. Hosted mode simply does not reuse it.

### User-visible failure mode

The hosted LLM may still be able to see tools about extensions, such as
`tool_search` or `tool_activate`, but it does not reliably see the actual
third-party MCP tools that were configured by the user. When it does see a
partial or reconstructed representation, it lacks the exact documentation and
schema shape that the MCP server originally declared.

That causes:

- malformed arguments
- omitted required fields
- enum mismatches
- poorer tool choice because descriptions are less specific
- confusion between "manage extensions" and "call the active tool itself"

## Goals

1. Expose the real call signatures for active, hosted-visible MCP tools to the
   hosted LLM.
2. Expose the real tool descriptions for those tools to the hosted LLM.
3. Reuse the canonical orchestrator-side tool metadata rather than duplicating
   MCP activation or schema generation logic in the worker.
4. Ensure every tool advertised to the hosted LLM is actually executable in
   hosted mode.
5. Preserve the existing `ToolDefinition` contract used by LLM providers.

## Non-goals

1. Replacing the MCP client implementation.
2. Replacing the existing `ToolDefinition` struct with a new provider-specific
   shape.
3. Exposing tools that hosted mode cannot safely execute.
4. Solving all approval and authentication UX problems for every remote tool in
   the same change.
5. Moving all tool execution into the worker container.

## Design Principles

1. The orchestrator owns extension activation, so it should also own the
   hosted-visible definitions for extension-provided tools.
2. The worker should not re-discover or re-activate MCP servers on its own.
3. The LLM should receive the original tool description and JSON Schema, not a
   lossy summary.
4. A tool must not be advertised unless the hosted execution path can actually
   run it.
5. Server-level MCP instructions may supplement the interface, but they must
   not replace per-tool descriptions and schemas.
6. The worker-orchestrator transport contract for hosted tool catalogue fetch
   and proxy execution must be owned in one shared boundary, not duplicated as
   parallel route fragments and payload conventions.

## Proposal

### 1. Introduce a hosted-visible orchestrator tool catalogue

Add a worker-authenticated orchestrator endpoint that returns the
orchestrator-owned tools that are visible and executable for hosted jobs.

Suggested route:

```text
GET /worker/{job_id}/tools/catalog
```

Suggested response shape:

```json
{
  "tools": [
    {
      "name": "notion_search",
      "description": "Search pages and databases in Notion.",
      "parameters": {
        "type": "object",
        "properties": {
          "query": {
            "type": "string",
            "description": "Search query"
          }
        },
        "required": ["query"]
      }
    }
  ],
  "toolset_instructions": [
    "The Notion MCP server searches only content visible to the authenticated workspace."
  ],
  "catalog_version": 7
}
```

The `tools` array is the LLM-facing contract. `toolset_instructions` is
optional supplemental context synthesized from server-level metadata such as MCP
`initialize` instructions. `catalog_version` exists for caching and refresh
decisions; it is not exposed to the LLM.

The important implementation constraint is that this route and its payload
types should be introduced through one shared worker-orchestrator transport
module or equivalent typed boundary. The hosted-catalog fix should not add a
second copy of route strings, request bodies, and response bodies that must be
kept in sync by convention alone.

### 2. Filter the catalogue to hosted-executable tools

The catalogue must not blindly dump the whole orchestrator registry into hosted
mode. It must filter to tools that are valid in the hosted environment.
That filter should be owned by the canonical `ToolRegistry` or adjacent tool
policy layer, not rebuilt inside HTTP adapters.

For v1, a tool is hosted-visible only if all of the following are true:

1. The tool is currently active in the orchestrator registry.
2. The tool is safe to invoke from a hosted worker.
3. The tool does not require an interactive approval flow that hosted workers
   cannot satisfy.
4. The tool's runtime dependencies are available in the orchestrator process.

For MCP tools specifically, this means:

- active server connection exists
- tool definition is available from the live wrapper
- approval semantics are compatible with hosted mode

If IronClaw later grows a hosted approval grant mechanism, the filter can be
relaxed. Until then, the catalogue should prefer correctness over breadth.
The current implementation keeps the hosted-visible source set to active MCP
tools only; later roadmap work extends that same canonical filter seam to
orchestrator-owned WASM tools.

### 3. Add a generic orchestrator-owned tool execution endpoint

The current hosted path already has a special endpoint for extension-management
meta tools. Generalize this to a remote tool execution endpoint for
orchestrator-owned tools.

Suggested route:

```text
POST /worker/{job_id}/tools/execute
```

Suggested request:

```json
{
  "tool_name": "notion_search",
  "params": {
    "query": "quarterly roadmap"
  }
}
```

Suggested response:

```json
{
  "output": {
    "content": "Found 4 matching pages..."
  }
}
```

This endpoint should execute against the canonical orchestrator-side
`ToolRegistry`, not a worker-side clone.

### 4. Register worker-local proxy wrappers for remote tools

Once the worker fetches the hosted-visible catalogue, it should register proxy
tools locally so the reasoning loop can keep using a single local
`ToolRegistry`.

Conceptually:

1. Worker starts with container-local tools.
2. Worker fetches the orchestrator catalogue.
3. Worker registers a `RemoteToolProxy` for each catalogue entry.
4. Each proxy reports the orchestrator-supplied `name`, `description`, and
   `parameters` unchanged.
5. Each proxy executes by calling `POST /worker/{job_id}/tools/execute`.

This keeps the worker-side reasoning loop simple while still making the LLM see
the correct interface.

### 5. Keep the LLM interface unchanged and canonical

The LLM should continue to receive the existing `ToolDefinition` shape:

```rust
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}
```

That is already the right contract. The problem is not that the structure is
too weak. Before roadmap item `1.1.1`, hosted mode populated it from the wrong
registry.

The LLM-visible fields should map as follows:

| LLM field | Source of truth | Notes |
| --- | --- | --- |
| `name` | active tool wrapper | Keep the exact registered tool name |
| `description` | tool wrapper `description()` | Preserve MCP docs |
| `parameters` | tool wrapper `parameters_schema()` | Preserve the JSON Schema |

No provider-specific restatement or prose reconstruction should sit between the
MCP tool wrapper and the LLM.

### 6. Surface supplemental MCP server instructions separately

MCP servers may provide general usage instructions during initialization. These
can improve tool selection, but they are not a substitute for per-tool docs.

Recommended treatment:

1. Keep per-tool `description` and `parameters` as the primary interface.
2. Collect server-level instructions from active hosted-visible MCP servers.
3. Add them as a short synthesized system message before tool selection, or as
   `toolset_instructions` returned by the catalogue and injected by the worker.

Example:

```text
Active remote tool guidance:
- The GitHub MCP server only accesses repositories visible to the configured account.
- The Notion MCP server may return workspace-scoped results rather than global search.
```

This gives the model behavioural context without distorting tool signatures.

### 7. Refresh the catalogue when tool availability changes

Tool availability is not static. The hosted worker should refresh the
catalogue
when:

- the job starts
- an extension is activated or removed
- the orchestrator reports a catalogue version change
- an MCP server with `listChanged` support announces tool-list changes

The minimal v1 approach is to fetch once at worker startup and again after any
successful extension-management action that could change active tools. A later
iteration can add explicit catalogue version checks or push invalidation.

## Detailed Interface

### Worker-facing orchestrator API

```text
GET  /worker/{job_id}/tools/catalog
POST /worker/{job_id}/tools/execute
```

New data types:

```rust
pub struct RemoteToolCatalogResponse {
    pub tools: Vec<ToolDefinition>,
    pub toolset_instructions: Vec<String>,
    pub catalog_version: u64,
}

pub struct RemoteToolExecutionRequest {
    pub tool_name: String,
    pub params: serde_json::Value,
}

pub struct RemoteToolExecutionResponse {
    pub output: ToolOutput,
}
```

These types are worker-orchestrator transport concerns. Only
`ToolDefinition` is exposed to the LLM. The route names and payloads should be
defined once at this boundary and reused by both the orchestrator and worker
implementations.

### LLM-visible interface

The hosted LLM should see a single merged tool list:

1. worker-local container tools
2. worker-local proxies registered from the orchestrator catalogue, which
   expose
   orchestrator-owned hosted-visible tools, including active MCP tools

The LLM should not need to know which side owns execution. Ownership stays an
implementation detail behind the proxy layer.

### Naming rules

Tool names must stay stable and collision-resistant. For MCP tools, keep the
existing prefixed registration convention:

```text
<extension_name>_<server_tool_name>
```

Examples:

- `github_search_repositories`
- `notion_search`
- `slack_post_message`

The catalogue endpoint must return exactly the registered name, not a display
name and not a reconstructed alias.

## Why This Is The Correct Boundary

This boundary matches existing ownership:

- the orchestrator already owns extension activation
- the orchestrator already owns active MCP client state
- the orchestrator already owns the canonical registry for extension-provided tools
- the worker already knows how to proxy LLM and tool-related calls back to the orchestrator

The change is therefore architectural alignment, not a new subsystem.

It also avoids the wrong alternatives:

- do not re-implement MCP activation inside the worker
- do not serialize a lossy prose description of remote tools
- do not make the LLM infer argument shape from examples alone
- do not duplicate schema shaping logic for hosted mode only

## Security And Approval Considerations

This RFC does not argue that every active tool should be exposed to hosted
workers. It argues that every hosted-visible tool should be exposed with its
real definition.

Required rules:

1. Never advertise a tool the worker cannot actually execute.
2. Preserve approval policy when proxying execution through the orchestrator.
3. Keep credentials and MCP sessions in the orchestrator process.
4. Treat worker tokens as authorization to invoke only the job-scoped hosted
   tool surface, not the full orchestrator registry.

If a tool requires approval and there is no hosted approval flow, exclude it
from the catalogue.

## Testing Strategy

The change needs both transport tests and behavioural tests.

### Unit tests

1. Catalog construction returns orchestrator-owned active MCP tool definitions
   with original descriptions and schemas.
2. Catalog filtering excludes approval-gated or otherwise uncallable tools.
3. Worker proxy registration preserves the orchestrator-provided
   `ToolDefinition` exactly.
4. Generic remote execution dispatches to the requested orchestrator-owned tool.

### Behavioural tests

1. Hosted worker with an active MCP tool advertises that tool in
   `available_tools`.
2. Hosted worker can execute a proxied MCP tool end-to-end through the
   orchestrator.
3. Hosted worker still exposes container tools and extension-management proxy
   tools.
4. Extension activation refreshes the hosted-visible tool list.

### Regression test target

The key regression to lock down is:

> A hosted LLM receives the same `name`, `description`, and `parameters` for an
> active MCP tool that the canonical orchestrator registry would expose in the
> normal in-process path.

That is the contract that fixes malformed tool calls.

## Migration Plan

1. Add worker/orchestrator transport types for the hosted tool catalogue and
   generic remote tool execution, with one shared contract owner for route
   builders and payload shapes.
2. Add orchestrator-side catalogue filtering against the canonical `ToolRegistry`.
3. Add worker-side remote proxy registration using the shared transport
   contract rather than worker-local path reconstruction.
4. Merge remote tool definitions into the worker reasoning context.
5. Add targeted tests for definition fidelity, execution routing, and
   contract parity between worker and orchestrator.
6. Optionally inject supplemental server-level instructions into the system
   prompt once the basic catalogue path is stable.

## Alternatives Considered

### Alternative 1: Keep hosted mode as-is and improve prompts

Rejected. Better prompting cannot compensate for missing schemas or inaccurate
tool descriptions.

### Alternative 2: Reconstruct MCP schemas into a hosted-specific summary

Rejected. This duplicates logic, drifts from the canonical registry, and risks
exactly the same class of schema/documentation mismatch that caused the current
problem.

### Alternative 3: Move all hosted `complete_with_tools` assembly into the orchestrator

This is a viable long-term simplification. It would remove the need for the
worker to fetch and register remote tool definitions at all.

It is not the recommended first step because the
catalogue-plus-proxy design is a smaller change that fits the current worker
architecture and can be adopted incrementally.

### Alternative 4: Stop for a separate worker-orchestrator architecture cleanup

Rejected. The contract duplication is real, but it sits directly on the path of
hosted tool catalogue delivery. The right response is to make the shared
transport boundary part of this RFC's first implementation step, not to block
the work behind a separate prerequisite stream.

## Open Questions

1. Should hosted mode expose only `ApprovalRequirement::Never` tools in v1, or
   also tools that are already auto-approved by policy?
2. Should `toolset_instructions` be injected as a dedicated system message, or
   folded into the job system prompt?
3. Should the catalogue be refreshed opportunistically after extension actions
   only, or version-checked every reasoning iteration?
4. Should remote orchestrator-owned tools appear in the UI as a separate source
   category for observability, even though the LLM sees a unified list?

## Recommendation

Implement the catalogue-plus-proxy design first.

It is the smallest change that fixes the actual bug:

- the LLM will see the correct MCP tool descriptions
- the LLM will see the correct JSON Schemas
- execution will still happen in the component that owns MCP sessions and
  activation state
- hosted mode will stop reasoning from extension-management meta tools when it
  should be calling the actual third-party tool

That is the correct fix because it treats the cause, not the symptom.
