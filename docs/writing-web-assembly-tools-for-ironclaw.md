# Writing WebAssembly Extensions for IronClaw

This document is adapted from
`../imap-wasm/docs/writing-web-assembly-tools-for-ironclaw.md` and updated to
match the IronClaw repository as it exists on 2026-03-09.

The most important correction is versioning: new or rebuilt IronClaw
extensions must target WebAssembly Interface Types (WIT) `0.3.0`. This is not
a speculative future version.
It is the current host contract used by both `wit/tool.wit` and
`wit/channel.wit`.

## Read This First

Three facts shape almost every extension design decision:

1. The authoritative contracts are [wit/tool.wit](/data/leynos/Projects/ironclaw/wit/tool.wit)
   and [wit/channel.wit](/data/leynos/Projects/ironclaw/wit/channel.wit).
1. The package line for both contracts is `package near:agent@0.3.0;`.
1. IronClaw installs WebAssembly extensions from named `.tar.gz` bundles, not
   from loose build directories.

That leads to a practical authoring checklist:

- build against the shared WIT files in this repository
- declare `wit_version: "0.3.0"` in the shipped capabilities sidecar
- treat host HTTP and host secret injection as the real outbound integration
  surface
- package `{name}.wasm` and `{name}.capabilities.json` inside a `.tar.gz`
  archive
- prove both component compatibility and runtime behavior in tests

## The Current WIT Format

IronClaw has two extension worlds:

- tools implement `world sandboxed-tool`
- channels implement `world sandboxed-channel`

Both live under the same package version:

```wit
package near:agent@0.3.0;
```

### Tools

The tool world imports `host` and exports `tool`.

The guest exports exactly three functions:

- `execute(req: request) -> response`
- `schema() -> string`
- `description() -> string`

The important shape detail is that this boundary is string-heavy by design:

- `request.params` is a JSON string
- `request.context` is an optional JSON string
- `response.output` is an optional JSON string
- `response.error` is an optional plain string

Do not design the guest around a richer typed RPC contract than that. Parse
JSON inside the guest, and make `schema()` truthful enough that the host and
LLM can tell when to call the tool.

### Channels

The channel world imports `channel-host` and exports `channel`.

The guest exports host-managed callbacks:

- `on-start(config-json: string) -> result<channel-config, string>`
- `on-http-request(req: incoming-http-request) -> outgoing-http-response`
- `on-poll()`
- `on-respond(response: agent-response) -> result<_, string>`
- `on-status(update: status-update)`

Like tools, channels cross the boundary with JSON strings where the contract
needs flexibility:

- HTTP headers and query parameters are JSON strings
- channel metadata is a JSON string
- attachment extras are a JSON string

This is deliberate. IronClaw uses JSON-bearing records to keep the WIT stable
while allowing extension-specific metadata to evolve.

## Versioning Rules That Matter In Practice

There are two separate version surfaces:

1. the WIT package version in the `.wit` file
1. the declared `wit_version` in the extension's capabilities JSON

For the current host, both must resolve to `0.3.0` for new or rebuilt
extensions.

For example:

```json
{
  "type": "tool",
  "name": "example-tool",
  "wit_version": "0.3.0"
}
```

```json
{
  "type": "channel",
  "name": "example-channel",
  "wit_version": "0.3.0"
}
```

Historical artifacts may still declare older values, and IronClaw's
compatibility tests intentionally keep some legacy coverage. That is not a
licence to ship new sidecars with `0.1.0` or `0.2.0`.

One subtle point from the repository tests:

- new extensions should target versioned package imports such as
  `near:agent/host@0.3.0` and `near:agent/channel-host@0.3.0`
- the host-side `wit_compat` tests still register both versioned and
  unversioned interface paths so genuinely old artifacts can instantiate during
  migration windows

That compatibility shim exists for old artifacts. It is not the target format
for new ones.

## Capability Reality Beats ABI Theory

The WIT files describe the ABI, but the host capability model is stricter than
"anything the component model could theoretically express".

### HTTP Is The Real Outbound Network Primitive

For tools and channels alike, outbound service integration is built around the
host `http-request` import. That is why HTTP-shaped integrations fit well and
guest-managed socket protocols fit poorly.

### `tool-invoke` Exists In WIT But Is Not A Safe Design Primitive Yet

`tool-invoke` is part of the tool host interface, but the current IronClaw
runtime does not make it a reliable composition mechanism for guest code. Do
not build a new extension assuming guest-to-guest tool orchestration is the
core happy path.

### Secrets Are Host-Injected, Not Guest-Readable

This rule is worth stating bluntly:

- the guest can check whether a secret exists
- the guest cannot read the secret value
- the host injects credentials into outbound HTTP requests when the sidecar
  configuration allows it

That means `setup.required_secrets` is appropriate for credentials the host
will later consume, but it is the wrong place for non-secret values the guest
must inspect directly.

## Packaging Contract

The installable artifact is a `.tar.gz` bundle containing files whose basenames
match the extension name.

For a tool named `example-tool`, the bundle should contain:

- `example-tool.wasm`
- `example-tool.capabilities.json`

For a channel named `example-channel`, the bundle should contain:

- `example-channel.wasm`
- `example-channel.capabilities.json`

Keeping an unpacked `dist/<name>/` directory is useful for inspection, but the
web UI and registry flows care about the archive.

## Testing: Prove The Right Thing At The Right Layer

One end-to-end (E2E) test is not enough for WebAssembly extensions.

At minimum, prove two separate things:

1. the built component instantiates against the current host linker
1. the extension behavior is correct at the protocol or feature level

IronClaw's current repo-level guardrail for the first point is
[tests/wit_compat.rs](../tests/wit_compat.rs). If the WIT shape,
versioning, or extension packaging changes, that matrix should be
rerun.

For the second point, use behavior tests that exercise the real request and
response semantics your extension depends on. Mock-only coverage is not enough
if the mock hides the integration boundary that matters.

## Recommended Author Workflow

1. Start from the shared WIT file in this repository rather than copying an old
   snapshot from another project.
1. Build the guest around JSON parsing and JSON Schema output instead of
   inventing a second typed protocol.
1. Declare a truthful `wit_version: "0.3.0"` in the sidecar.
1. Keep secrets at the host boundary and pass non-secret guest-consumed values
   as normal request parameters.
1. Package a `.tar.gz` with names that match the installation name exactly.
1. Run component compatibility tests and behavior tests before shipping.

## Related Repo Docs

- [docs/BUILDING_CHANNELS.md](/data/leynos/Projects/ironclaw/docs/BUILDING_CHANNELS.md)
- [src/tools/README.md](/data/leynos/Projects/ironclaw/src/tools/README.md)
- [wit/tool.wit](/data/leynos/Projects/ironclaw/wit/tool.wit)
- [wit/channel.wit](/data/leynos/Projects/ironclaw/wit/channel.wit)
- [tests/wit_compat.rs](../tests/wit_compat.rs)
