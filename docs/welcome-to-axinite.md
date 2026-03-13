# Welcome to Axinite

Axinite is a fork of [IronClaw](https://github.com/nearai/ironclaw) – a
Rust-based personal AI assistant runtime built on defence-in-depth
isolation for tools and secrets. IronClaw itself draws directly from
[OpenClaw](https://github.com/openclaw/openclaw) (formerly Moltbot,
formerly Clawdbot), the TypeScript original with ~309k GitHub stars and
a broad ecosystem of messaging integrations.

Axinite inherits IronClaw's architecture and its core covenant: the
assistant works for the user, keeps data local, remains auditable, and
treats security as a product feature rather than a bolt-on. Where
Axinite diverges is in what it optimizes for. IronClaw tracks feature
parity with OpenClaw across a wide surface. Axinite rejects that goal.
The fork chooses depth over parity: a deliberately small set of
capabilities, each implemented completely, performantly, and safely.

## Mission

> Axinite exists to deliver a security-first, local-first personal
> agent runtime that ships a deliberately narrow capability surface –
> MCP over HTTPS, WIT/wasmtime extensions with provenance-aware intent
> enforcement, a constrained Python codemode, associative memory with
> decay and reinforcement, first-class OpenAI Responses over WebSocket,
> and multi-file skill bundles – and implements each one to completion.
>
> Every boundary is explicit: capabilities, secrets, network,
> provenance, and approvals. Every boundary is exposed through a
> responsive, localized, accessibility-first UI on Linux, PWA, and
> Telegram – so the assistant remains auditable, trustworthy, and
> unambiguously on the user's side.

## Heritage in brief

Three projects share a common architectural spine: channels feed
messages into a control plane (the "Gateway"), which mediates sessions,
memory, tool invocation, and UI surfaces.

```text
  OpenClaw (TypeScript)
      │
      ▼
  IronClaw (Rust reimplementation, security-first)
      │
      ▼
  Axinite (fork: depth over parity)
```

**OpenClaw** is the broadest implementation. Gateway hub, dozens of
messaging channels (WhatsApp, Telegram, Slack, Discord, and more),
companion apps for macOS/iOS/Android, a web UI, and a daemon that stays
running as a user service. It treats inbound DMs as untrusted input by
default and requires explicit pairing before an unknown sender can
interact with the assistant.

**IronClaw** rewrites the runtime in Rust with a single-binary
deployment model and a stricter isolation story. Untrusted tool code
runs in a WebAssembly sandbox with capability-style permissions. Secrets
never touch extension code – the host injects credentials at request
time. A leak scanner inspects both outbound requests and inbound
responses. Persistence targets PostgreSQL with pgvector (optional
libSQL).

**Axinite** starts from IronClaw's codebase. At the time of writing
(March 2026), the fork has no releases and no visible user base.
Internal artefacts (crate name, README branding) still identify as
IronClaw. Expect rough edges.

## Why a fork

A fork implies divergence that can't be reconciled through pull
requests and feature flags. Axinite's changes meet that threshold in
three areas.

**The trust model changes at the WASM boundary.** IronClaw's current
WASM harness supports placeholder-based secret substitution:
extensions choose where secret material lands via `{TOKEN}`-style
markers in URLs and headers. Axinite's provenance/zero-knowledge
intent RFC disables that mechanism entirely for ZK-mode tools and
replaces it with a structured intent ABI where extensions declare
*what* they want to do and the host assembles every HTTP request,
injects credentials only into harness-controlled sinks, and enforces
noninterference-like constraints via runtime provenance tracking and
a Rego policy engine. That's not a hardening patch. It's a different
contract between extensions and the runtime – one that existing
IronClaw extensions would need to adapt to.

**The memory subsystem becomes a separate process.** IronClaw's
memory is in-process: PostgreSQL tables with hybrid full-text and
vector search fused via Reciprocal Rank Fusion. Axinite proposes a
local-only Rust sidecar (`memoryd`) that consumes a transactional
outbox from PostgreSQL, performs extraction via Ollama structured
outputs, stores vectors in Qdrant, stores normalized facts and
relations in Oxigraph (embedded, no network exposure), and runs
consolidation jobs through Apalis workers. The sidecar communicates
over a Unix domain socket with capability-token authentication.
Bolting an event-driven, multi-store memory architecture with its own
process boundary onto an upstream that hasn't asked for it would be
presumptuous at best.

**The tool lifecycle inverts a core assumption.** IronClaw's WASM
wrapper treats schema disclosure as a reactive retry hint on tool
failure – the model discovers the interface by getting it wrong first.
Axinite's MCP and WASM tool definition RFCs invert this to a
proactive guarantee: every active tool exposes its canonical schema
before first invocation, delivered through a hosted-visible
orchestrator tool catalogue with proxy execution. The existing retry
hint path demotes to fallback diagnostics. That's an opinionated
change to the tool-calling contract that would need upstream
philosophical agreement before it could land as a contribution.

Seven RFCs define the fork's design direction. Each proposes changes
that are internally consistent but collectively reshape assumptions
the upstream codebase currently depends on. Contributing them
piecemeal would fragment the design; maintaining them as a coherent
fork preserves the ability to validate them as a system.

## The capability surface

Axinite's scope is bounded by design. Seven subsystems define the
runtime's ambition; everything else is subordinate.

### MCP over HTTPS

The Model Context Protocol is Axinite's primary tool substrate. The
commitment goes beyond "it connects" – MCP over HTTPS requires
concrete hardening: `Origin` validation to mitigate DNS rebinding,
localhost binding for local-only servers, and proper authentication
on every transport.

Axinite treats accurate tool schemas as essential, not optional. The
MCP tool definitions RFC introduces a hosted-visible orchestrator
tool catalogue (`GET /worker/{job_id}/tools/catalog`) that exposes
real `ToolDefinition` values – name, description, and JSON Schema –
for every active MCP tool. Hosted workers register proxy wrappers
locally and execute through a generic orchestrator endpoint
(`POST /worker/{job_id}/tools/execute`). The LLM receives the
original tool description and schema, not a lossy summary or a
post-failure hint.

The catalogue filters to hosted-executable tools only: active server
connection, available definition, approval semantics compatible with
hosted mode. Tools that can't actually run don't get advertised.
Server-level MCP instructions supplement per-tool documentation as
contextual guidance, never as a replacement for machine-readable
schemas.

### WIT/wasmtime extensions with provenance-aware intent enforcement

The WebAssembly Component Model, via WIT interface definitions and
the wasmtime runtime, provides capability boundaries where the host
owns enforcement. Axinite extends IronClaw's existing `sandboxed-tool`
world with a structured intent ABI.

The provenance/zero-knowledge intent RFC redesigns the trust contract
between extensions and the harness. Extensions declare intents – what
they want to do – as structured WIT resources with semantic template
identifiers, typed arguments, and effect declarations. The harness
renders intents for user understanding, evaluates them against a Rego
policy engine, assembles concrete HTTP requests, and injects
authentication only at send time into harness-controlled sinks.
Results return with provenance metadata as opaque tokens that track
data origin (network-derived, user-supplied, constant) through the
execution.

Placeholder-based secret substitution – where extensions control where
secret material lands – is disabled entirely for ZK-mode tools. The
harness enforces a noninterference-like constraint: secret-derived or
remote-derived values cannot flow into public sinks without explicit
approval. Literal token leak detection (IronClaw's existing
Aho–Corasick scanner) remains, but provenance tracking addresses the
harder problem of derived data flows through indirect channels.

The delegated endpoint mechanism fits this model: extensions reference
an opaque endpoint name (e.g. `"jmap-default"`) and the host resolves
the real address, enforces policy, injects credentials, and redacts
the concrete URL from agent-visible surfaces and logs.

The WASM tool definitions RFC complements this by guaranteeing that
every active WASM tool exposes its canonical parameter schema before
first invocation, sourced from guest-exported `schema()` or explicit
host override. The existing schema-as-retry-hint path demotes to
supplemental diagnostics. Hosted workers receive orchestrator-owned
WASM tool definitions through the same remote-tool catalogue used for
MCP tools.

### Constrained Python codemode (Monty)

Tool calls handle simple actions adequately. They fall apart when an
agent needs loops, branching, or intermediate state.

Axinite embeds [Monty](https://github.com/pydantic/monty), a minimal
Rust-hosted Python interpreter, as a codemode runner. The surface is
deliberately narrow:

```text
save_script(name, code, allowed_tools, entrypoint="main")
run_script(name, params, allowed_tools, state=None)
exec_code(code, allowed_tools, params=None, state=None)
```

Each invocation receives an explicit tool allowlist and communicates
through a JSON-only ABI. The runner executes in a subprocess – a Monty
panic won't bring down the parent process. Filesystem, environment,
and network access are blocked by default; only capabilities granted
by the host are available. Full CPython compatibility and package
installation are explicit non-goals. The value proposition depends on
the boundary staying legible and enforceable.

Monty is experimental. Its maintainers say as much. Treat the
runtime's maturity accordingly.

### Associative memory (memoryd sidecar)

IronClaw ships persistent memory primitives: hybrid search across
`memory_documents` and `memory_chunks` in PostgreSQL, fused via
Reciprocal Rank Fusion. Axinite replaces this with a dedicated
local-only Rust sidecar.

The memoryd RFC specifies a concrete architecture:

- A **transactional outbox** in PostgreSQL, populated inside the same
  database transaction as conversation message and workspace document
  writes. No "message without state" or "state without message"
  failure modes.
- A **local sidecar process** that consumes outbox events, performs
  entity/fact extraction via Ollama structured outputs, stores
  retrieval vectors in **Qdrant** (collection-per-workspace for
  isolation and deletion correctness), and stores normalized facts and
  relations in **Oxigraph** (embedded as a library crate – no SPARQL
  HTTP server, no network exposure).
- **Capability-token enforcement** over a Unix domain socket RPC
  surface. Short-lived tokens minted per request with minimal scope,
  split between read (`recall`, `readfacts`) and write (`ingest`,
  `reinforce`, `retract`, `purge`). Socket permissions 0600, owned by
  the IronClaw user.
- **Apalis workers** for consolidation: extraction, concept upsert,
  fact upsert, workspace consolidation (clustering, promotion, decay),
  and deletion reconciliation – all with retries, timeouts, and
  concurrency limits backed by PostgreSQL queues.
- **Decay and reinforcement**: candidate facts promote to stable after
  appearing in multiple episodes or receiving user reinforcement.
  Strength decays exponentially over time. Conflicting facts coexist
  as candidates until one dominates by recency, reinforcement, or
  curation. Retraction marks facts rather than hard-deleting, with
  propagation through the provenance chain.
- **Phased rollout**: Disabled → Shadow (populate stores, log only,
  compare against baseline) → Active (memoryd serves recall, with
  fallback to existing workspace search).

### OpenAI Responses over WebSocket

IronClaw's existing provider stack speaks Chat Completions: the
`LlmProvider` trait exposes synchronous `complete` and
`complete_with_tools` methods with no streaming interface. Axinite's
WebSocket Responses RFC proposes a new provider protocol that
addresses three capabilities the current adapter can't accommodate.

**Stateful continuation.** WebSocket mode maintains a connection-local
cache of the most recent previous-response state. Axinite treats the
Responses backend as a stateful session object owned by the
agent-loop delegate – one WebSocket connection per active thread –
with explicit policy for reconnect and resume, including handling of
`previous_response_not_found` in `store=false` mode after
disconnection.

**Tool lifecycle fidelity.** The Responses API uses `call_id` as the
join key for `function_call_output` items. IronClaw currently
synthesizes tool IDs when serializing turns (`turn{n}_{i}`), which
works for Chat Completions but fails for Responses because OpenAI
validates the linkage. The RFC requires storing provider-owned call
identifiers per tool call, not reconstructing them later.

**Server-side compaction.** The `context_management` +
`compact_threshold` mechanism emits opaque encrypted compaction items
into the response stream. These items must be persisted as-is and
included in subsequent requests when chaining statelessly. When using
`previous_response_id`, manual pruning is forbidden. The RFC
explicitly addresses the interaction between server-side compaction
and Axinite's existing native summarization, recommending that native
compaction be disabled or demoted to fallback when the Responses
backend is active.

### Skill bundles

IronClaw's current skill system is effectively single-file: ZIP
installation extracts only the root `SKILL.md` and discards ancillary
content. The skill bundle RFC introduces a first-class multi-file
format.

A `.skill` file is a ZIP archive with `SKILL.md` at the root and
optional `references/` and `assets/` directories. The installer
validates against a strict allowlist: no `scripts/` or `bin/`
directories, no symlinks, no traversal paths, no executable file
extensions. A skill bundle is documentation plus passive assets, not
a plugin or command package.

The runtime exposes a `skill_read_file` tool – read-only, skill-scoped,
path-validated – so the model can access bundled reference material
on demand without general filesystem access. The prompt contract
follows progressive disclosure: advertise the skill identifier, inject
only `SKILL.md` on activation, load ancillary files lazily when
referenced. Binary assets and oversized files return a typed non-inline
error with metadata rather than content.

Installation supports HTTPS URL and direct upload, with canonical
name derivation, version-aware upgrade semantics, and collision
handling. Single-file `SKILL.md` installs remain supported unchanged.

### Responsive, localized, accessible UI

Axinite targets Linux, PWA, and Telegram. That's a pragmatic
constraint, not a strategic one: the fork has a single maintainer,
and delivering one channel well matters more than delivering ten
badly. Telegram is the starting point because its bot API is open,
well-documented, and doesn't gatekeep behind platform review
processes – the right substrate for fast iteration. Additional
channels (WhatsApp, Signal, Discord, and more exotic ingresses)
follow once the first is solid.

The accessibility commitment is structural, not cosmetic. WCAG 2.2
defines the target: perceivable, operable, understandable, robust.
Keyboard navigation, assistive-technology interoperability, and
language negotiation are designed in from the outset, not retrofitted.

## Security model

Axinite inherits IronClaw's layered approach. The mental model for
tool execution:

```text
WASM ──► Allowlist ──► Leak scan ──► Credential ──► Execute ──► Leak scan ──► WASM
         validator     (request)     injector       request     (response)
```

The system assumes extensions can be malicious or compromised. Every
outbound request passes through allowlist validation and leak detection
before credentials are injected. Every response is scanned before it
returns to extension code.

The fork's additions reinforce this posture at specific weak points:

- **Provenance/ZK intents**: runtime taint tracking with Rego policy
  enforcement. Placeholder-based secret substitution disabled for
  ZK-mode tools. Extensions declare intents; the host assembles every
  HTTP request. Noninterference-like constraints on data flow between
  secret-derived sources and public sinks.
- **Monty runner**: capability-brokered execution with a per-run tool
  allowlist and subprocess isolation. No ambient authority.
- **Delegated endpoints**: URL confidentiality enforced at the host.
  Extensions operate on names, never addresses.
- **memoryd**: capability-token authentication over UDS. Qdrant and
  Ollama bound to loopback. Oxigraph embedded with no network
  exposure. Leak detector runs on all content before ingestion.
- **Skill bundles**: executable content rejected at install time.
  Model access to bundled files is read-only and skill-scoped.

No telemetry, analytics, or data sharing. All data stays local.

## Design guardrails

The mission statement only helps if it produces exclusion criteria –
reasons to say "no" when a feature looks attractive.

**No feature parity drift.** IronClaw's parity matrix coordinates work
against OpenClaw's catalogue. Axinite measures success differently:
implementation excellence on a narrow surface trumps completeness
against a broader upstream.

**Capability-mediated boundaries are non-negotiable.** The difference
between tooling that works and tooling that can be trusted with real
accounts lives in the boundary enforcement. Opaque endpoint
identifiers, host-resolved credentials, structured intent declarations,
provenance-aware policy – these aren't optional hardening. They're the
product.

**Schemas before execution.** Models need canonical tool definitions
up front. Recovering from malformed calls via error hints wastes
tokens, harms reliability, and trains bad habits into the interaction
loop.

**Programmability stays constrained and auditable.** Monty's value is
the boundary, not the language. External-function-only I/O plus
resource limits let Axinite offer rich control flow without becoming a
general-purpose scripting host with an unbounded attack surface.

**Accessibility is product quality, not polish.** WCAG compliance,
keyboard navigation, and localization are planned from day one. A
PWA-first UI that fails accessibility review has failed its mission.

**Scope expands only when it strengthens the core.** Ancillary
features – Canvas hosting, systemd integration, health monitoring,
enhanced CLI, model switching, `llms.txt` discovery – graduate from
"nice to have" to "in scope" only when they strengthen
security/auditability or materially improve the usability of the core
subsystems.

## Ecosystem context

Axinite doesn't exist in isolation. Knowing the neighbours helps.

- **OpenClaw** remains the most actively developed project in the
  lineage (~309k stars, ~58.7k forks, releases on a near-daily
  cadence). Broadest integration surface, largest community.
- **IronClaw** tracks feature parity with OpenClaw from a Rust
  baseline (~9.8k stars, 21 releases, latest 0.18.0 as of
  March 2026).
- **MCP (Model Context Protocol)** is the JSON-RPC-based open protocol
  both IronClaw and Axinite use for connecting to external tool
  servers. The spec makes explicit that MCP enables arbitrary data
  access and code execution paths – consent and control are
  first-order concerns, not afterthoughts.
- **Monty** is maintained by Pydantic. Experimental. Blocks
  filesystem/env/network by default, exposes only host-granted
  functions. The "start from nothing" model is the right instinct for
  agent-executed code, but treat the runtime's maturity accordingly.
- **wasmtime** is the Bytecode Alliance's WebAssembly runtime,
  supporting WASI and the Component Model. Axinite uses it to host
  sandboxed extensions as compiled WASM components instantiated via
  linkers – the enforcement layer beneath the capability boundary.
- **WIT** (WebAssembly Interface Type) defines the contracts –
  interfaces and "worlds" – that WASM extensions declare. WIT
  describes capability boundaries, not behaviour; the host owns
  enforcement. Axinite's intent ABI builds directly on this
  separation, using WIT resources for opaque provenance tokens.
- **Qdrant** is a Rust-native vector similarity search engine with
  HNSW indexing, hybrid dense/sparse search, and payload filtering
  applied during traversal. The memoryd sidecar uses Qdrant for
  episode and concept retrieval, with collection-per-workspace
  isolation.
- **Oxigraph** is a SPARQL-compliant graph database written in Rust,
  backed by RocksDB. It implements RDF 1.2 and SPARQL 1.2 and embeds
  as a library crate. The memoryd sidecar uses Oxigraph for
  normalized fact and relation storage with named graphs per
  workspace.
- **Agent Skills** is an open standard (originated by Anthropic, now
  adopted by OpenAI, Microsoft, GitHub, Cursor, and others) defining
  a portable format for packaging agent capabilities as directories
  of instructions, scripts, and resources. Skills use progressive
  disclosure – metadata at discovery time, full instructions on
  activation, reference files on demand – to manage context window
  pressure. Axinite's skill bundle format aligns with this standard
  at the schema and discovery layer.

## Further reading

- [OpenClaw repository](https://github.com/openclaw/openclaw) –
  Gateway architecture, DM pairing defaults, channel integrations
- [IronClaw repository](https://github.com/nearai/ironclaw) –
  security-first design, feature parity matrix, release cadence
- [MCP specification](https://modelcontextprotocol.io) – protocol
  model, trust and safety framing
- [Monty repository](https://github.com/pydantic/monty) –
  capability-based Python execution, explicit experimental status
- [wasmtime](https://wasmtime.dev/) – Bytecode Alliance WASM runtime,
  WASI and Component Model support
- [WIT specification](https://component-model.bytecodealliance.org/design/wit.html) –
  interface types for the WebAssembly Component Model
- [Qdrant](https://qdrant.tech/) – Rust-native vector search engine,
  HNSW indexing, hybrid search
- [Oxigraph](https://github.com/oxigraph/oxigraph) – SPARQL graph
  database in Rust, RDF 1.2 support, embeddable as a crate
- [Agent Skills standard](https://agentskills.io/specification) –
  open format specification for portable agent capabilities
- [OpenAI Responses WebSocket docs](https://platform.openai.com/docs/guides/websocket) –
  transport, compaction, and continuation semantics
- [WCAG 2.2](https://www.w3.org/TR/WCAG22/) – accessibility success
  criteria
- Axinite RFCs in `docs/rfcs/` – Monty integration, delegated
  endpoints, provenance-based ZK intents, secure memory sidecar, MCP
  tool definitions, WASM tool definitions, skill bundle installation
