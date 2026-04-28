# User's guide

This guide captures operator-visible behaviour for the current Axinite
runtime.

## Skill bundle installs

Axinite now validates passive multi-file skill bundles when a skill is
installed from an HTTPS URL or catalogue download that resolves to a `.skill`
ZIP archive, or when a local `.skill` file is uploaded through the browser
Skills tab.

A valid bundle must contain exactly one shared top-level path prefix with
`SKILL.md` at `<root>/SKILL.md`. The installer also accepts optional
`references/` and `assets/` subdirectories under that same root.

The installer rejects bundles that contain unsupported top-level content,
nested `SKILL.md` files, `scripts/` or `bin/` directories, traversal paths,
absolute paths, special-file entries, duplicate normalized paths, oversized
entries, oversized archives, excessive file counts, invalid UTF-8 in
`SKILL.md` or `references/`, or executable payloads such as `.sh`, `.py`,
`.js`, `.ps1`, `.bat`, `.cmd`, `.rb`, or `.pl`.

Accepted bundles are extracted through a staged install path and only committed
into the installed-skills directory once validation and on-disk staging both
succeed. A failed bundle install should therefore leave no partial installed
skill tree behind.

The browser Skills tab accepts exactly one install source at a time: a
catalogue name or slug, an HTTPS `SKILL.md` or `.skill` URL, raw `SKILL.md`
content from an API client, or one uploaded `.skill` file. Ambiguous requests,
such as sending both a name and a URL, fail before any download or install
attempt. Malformed archives report explicit `invalid_skill_bundle: ...` errors
that describe the archive-shape problem.

This slice does not yet add runtime file reads from bundled references and
assets. When a bundled skill is active, the runtime still injects only the
selected `SKILL.md` body into the prompt.

## Hosted workers and remote tools

Hosted workers now advertise two tool families to the model:

- container-local development tools such as `shell`, `read_file`,
  `write_file`, `list_dir`, and `apply_patch`
- active hosted-visible orchestrator-owned tools fetched from the
  worker-authenticated remote catalogue at startup, including both Model
  Context Protocol (MCP) tools and WebAssembly (WASM) tools

When the worker receives the remote catalogue, it registers a local proxy for
each advertised remote tool. The model therefore sees the orchestrator-owned
tool's `name`, `description`, and JSON Schema parameters unchanged, even though
execution still happens in the orchestrator process.

If one of those hosted-visible WASM tools later fails, any retry guidance is
supplemental recovery help only. The worker and orchestrator still treat the
advertised `ToolDefinition.parameters` schema as the primary contract for the
first call. The same proactive first-call contract now applies in both the
local in-process path and the hosted worker path: the first tool-capable model
request already carries the advertised WASM schema before any execution
attempt.

The worker now computes one registry-backed tool surface for reasoning. That
merged view is used both when the initial reasoning context is built and when
later hosted-loop iterations refresh `available_tools`, so long-running jobs
do not drift back to a local-only view. Supplemental hosted guidance from the
catalogue is injected once as a dedicated system message during context build;
later refreshes update only the tool list and any queued follow-up prompts.

### Minimal workflow example

```text
1. Worker starts and fetches the hosted-visible remote catalogue.
2. Worker registers one local proxy per advertised remote tool.
3. Model selects a hosted-visible remote tool such as `github_search`.
4. Worker sends one generic remote-tool execution request:
   POST /worker/{job_id}/tools/execute
   {
     "tool_name": "github_search",
     "params": { "query": "hosted worker transport" }
   }
5. Orchestrator validates the params against the advertised schema, executes
   the tool, and returns the normal `ToolOutput` payload.
```

The hosted-visible remote catalogue is now filtered from the canonical
`ToolRegistry` rather than from the HTTP adapter layer. That catalogue now
includes active hosted-visible MCP tools plus active hosted-visible
orchestrator-owned WASM tools that are executable in hosted mode.
Extension-management built-ins and other ineligible orchestrator-owned tools
remain outside the hosted-visible catalogue.

Not every tool in the orchestrator registry is visible in hosted mode. Tools
may still be hidden when they:

### Visibility rules & defaults

| Case | Default visibility | Expected behaviour when selected |
| --- | --- | --- |
| Requires interactive approval | Hidden | Omitted from the remote catalogue because hosted workers cannot satisfy interactive approval prompts |
| Depends on worker-local execution | Hidden | Omitted because the orchestrator cannot safely proxy a worker-local dependency |
| Other ineligible, protected, or non-hosted-visible cases | Hidden | Omitted from the remote catalogue and rejected if called directly |
| Active hosted-visible MCP tool | Visible | Advertised unchanged in the remote catalogue and executed through the generic remote-tool request |
| Active hosted-visible orchestrator-owned WASM tool | Visible | Advertised unchanged in the remote catalogue and executed through the same generic remote-tool request |

If a hosted-visible remote tool is selected, the worker sends one generic
execution request to the orchestrator rather than using tool-family-specific
proxy routes.

## Self-repair notifications

Axinite can emit proactive self-repair notices when a background job remains in
the explicit `stuck` state long enough to cross the configured repair
threshold.

The repair loop currently checks only jobs that the runtime has already marked
`stuck`. Once a job has remained in that state for at least
`AGENT_STUCK_THRESHOLD_SECS`, the background repair task may attempt recovery
and broadcast a notification such as:

- `Self-Repair: Job <id> was stuck for <n>s, recovery succeeded: ...`
- `Self-Repair: Job <id> was stuck for <n>s, recovery failed permanently: ...`
- `Self-Repair: Job <id> needs manual intervention: ...`
- `Self-Repair: Tool '<name>' repaired: ...`
- `Self-Repair: Tool '<name>' needs manual intervention: ...`

These notices are advisory. A success message means the runtime moved the job
back into its normal retry path. A permanent failure or manual-intervention
message means the runtime could not finish recovery automatically, and the
operator should inspect the job or tool state before retrying work.

## Workspace memory search

When workspace memory is enabled, the search backend differs by database:

- **PostgreSQL** — performs pgvector cosine-distance queries directly.
- **libSQL / Turso** — attempts an indexed `vector_top_k(...)` query when a
  compatible fixed-dimension vector index exists. After the V9 migration
  (which removed the fixed-dimension index in favour of flexible-dimension
  vector storage, with `memory_chunks.embedding` stored as a
  flexible vector), the backend automatically falls back to brute-force
  cosine similarity computed in Rust. Results from both paths feed into the
  same Reciprocal Rank Fusion (RRF) pipeline, so hybrid full-text search
  (FTS) + vector retrieval is preserved.

To determine which search mode is active for a workspace, run:

```text
ironclaw doctor
ironclaw status
```

Both commands report whether indexed or brute-force vector retrieval is
currently in use. See `docs/database-integrations.md` for backend trade-offs
and performance considerations.
