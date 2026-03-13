# RFC: Tokenized Delegated Authorized Endpoint Requests

- Status: Proposed
- Date: 2026-03-11
- Target: WASM tools, extension management page, web gateway, security model
- Authors: Codex draft for review

## Summary

IronClaw should add a new delegated endpoint model for WebAssembly (WASM)
extensions so a
user can configure a service endpoint in the extension management page without
exposing that endpoint URL to either:

- the agent,
- the extension code,
- or the extension's static HTTP allowlist.

The motivating example is a hypothetical JSON Meta Application Protocol (JMAP)
WASM extension. A user should be able to configure a JMAP endpoint once in the
web UI. The extension should then receive only an opaque endpoint identity such
as `primary` or `jmap-default`. When the tool needs to check email, it should
call a new
host-managed authorized request service using that identity. IronClaw should
then:

1. resolve the opaque identity to a host-owned endpoint binding,
2. apply authorization and internal network policy,
3. inject any required credentials,
4. perform the request,
5. redact the concrete endpoint from logs, errors, and agent-visible surfaces.

This RFC proposes:

- a typed extension setup schema instead of the current secret-only setup model,
- a new endpoint binding store and service layer,
- a new WASM capability and WebAssembly Interface Types (WIT) host call for
  delegated endpoint requests,
- redaction and approval changes so endpoint URLs remain confidential,
- compatibility rules so existing raw URL `http_request` users continue working.

## Problem

The current IronClaw WASM model is built around one assumption:

every configurable extension setup field is a secret, and every HTTP request is
authorized against a guest-visible raw URL.

That assumption is visible in four places:

1. tool setup schemas expose only `setup.required_secrets`,
2. the web extension setup data transfer objects (DTOs) accept and return only
   `secrets`,
3. the frontend renders every setup field as a password input,
4. the WASM HTTP host call accepts a raw `url` and authorizes via
   `http.allowlist` plus credential `host_patterns`.

That model works for API keys and OAuth secrets. It does not work for the
requested JMAP flow.

### Why the current model fails for JMAP endpoint confidentiality

If the extension must not know the JMAP endpoint URL:

- the current `http_request(method, url, ...)` host call is unusable, because
  the guest must provide the raw URL,
- the current `http.allowlist` model is unusable, because the extension's
  capabilities file must name the real host,
- the current `CredentialMapping.host_patterns` join is unusable, because
  credential injection is keyed by the visible request host,
- the current setup UI is unusable, because it only knows how to store secrets,
  not host-owned delegated endpoint bindings.

The required model instead splits between:

- guest authority: "use authorized endpoint `jmap-default`",
- transport authority: "IronClaw resolved that to
  `https://mail.example.com/jmap/`, confirmed policy, injected credentials, and
  sent the request".

## Goals

- Let a user configure a per-extension delegated endpoint in the extensions UI.
- Keep the concrete endpoint URL hidden from the agent and the extension.
- Remove the requirement that the extension's static capabilities file name the
  real service host in `http.allowlist`.
- Preserve a fail-closed outbound security model.
- Keep existing raw URL WASM HTTP support for extensions that still need it.
- Generalize the design beyond JMAP so other provider-bound extensions can use
  the same surface.

## Non-Goals

- Full dynamic network access for extensions.
- Letting the agent inspect or edit the hidden endpoint URL after setup.
- Replacing the existing `http_request` host call for all extensions.
- Solving every provider-specific discovery problem in phase one.
- Sharing delegated endpoint bindings across unrelated users by default.

## Current Surface

### 1. Setup is secret-only

Today:

- tool setup schema is `setup.required_secrets`,
- setup API is `ExtensionSetupResponse { secrets }`,
- setup submission is
  `ExtensionSetupRequest { secrets: HashMap<String, String> }`,
- setup persistence path writes submitted values into `SecretsStore`.

This means a non-secret field like `jmap_endpoint_url` does not have a valid
home in the current model.

### 2. WASM HTTP is raw URL based

Today:

- WIT exposes `http-request(method, url, headers-json, body, timeout-ms)`,
- host authorization validates the raw URL against `capabilities.http.allowlist`,
- credential injection resolves secrets via `CredentialMapping.host_patterns`,
- redaction focuses on credential values, not confidential endpoint URLs.

This means the transport authority is keyed entirely by the concrete guest
request URL.

## Proposal

Introduce a new delegated endpoint model with four coordinated changes:

1. typed extension setup fields,
2. endpoint binding persistence and service APIs,
3. delegated authorized request capability for WASM,
4. endpoint-confidential logging and approval behaviour.

## Design Overview

### High-level flow

```text
User configures JMAP endpoint in Extensions UI
    -> IronClaw validates and stores encrypted endpoint binding
    -> Extension marked configured, but URL is never surfaced back

Agent asks JMAP tool to check email
    -> Tool calls delegated host request with endpoint_name = "jmap-default"
    -> IronClaw resolves endpoint binding internally
    -> IronClaw injects credentials and enforces internal policy
    -> Request executes
    -> Tool receives response body only, not the endpoint URL
```

### Core idea

Keep two outbound request surfaces:

1. `http_request(url, ...)`
   For existing extensions that use explicit raw URLs and explicit
   `http.allowlist`.

2. `authorized_endpoint_request(endpoint_name, request_spec)`
   For extensions that must not see the endpoint URL.

The second path is additive, not a breaking replacement.

### Identifier terminology

This RFC should use `endpoint_name` as the single caller-visible identifier for
delegated endpoints.

- The setup schema defines an `endpoint_name`.
- Capability grants authorize an `endpoint_name`.
- Credential bindings attach to an `endpoint_name`.
- The WIT host call receives an `endpoint-name`.
- Storage persists an `endpoint_name` on the binding record.

The word `binding` should refer only to the host-owned stored record, not to a
second identifier type.

## Frontend and Web API Changes

### Replace secret-only setup DTOs with typed setup fields

Current setup DTOs should evolve from:

```text
ExtensionSetupResponse { secrets: Vec<SecretFieldInfo> }
ExtensionSetupRequest { secrets: HashMap<String, String> }
```

to something closer to:

```text
ExtensionSetupResponse {
  name,
  kind,
  fields: Vec<ExtensionSetupField>,
}

ExtensionSetupRequest {
  values: HashMap<String, ExtensionSetupValue>,
}
```

Suggested field model:

```text
ExtensionSetupField =
  | SecretField
  | DelegatedEndpointField

SecretField {
  key,
  label,
  optional,
  provided,
  auto_generate,
}

DelegatedEndpointField {
  endpoint_name,
  label,
  endpoint_kind,      // e.g. "jmap"
  optional,
  configured,
  help_text,
}
```

Suggested value model:

```text
ExtensionSetupValue =
  | { type: "secret", value: "..." }
  | { type: "delegated_endpoint", url: "https://..." }
```

### UI behaviour

The extensions page should stop assuming that all setup fields are password
inputs.

For delegated endpoint fields:

- render a URL input,
- explain that the URL is stored for host-side use only,
- after save, show only `Configured` or `Needs attention`,
- never echo the stored URL back into the page,
- allow explicit replace or clear actions without revealing the current value.

Suggested copy:

> JMAP endpoint URL. IronClaw stores this for host-side authorized requests.
> The extension and the agent will not see the saved URL.

### Validation UX

On save:

- require `https://`,
- normalize and canonicalize the URL,
- optionally perform provider-specific validation,
- store only after validation succeeds,
- return sanitized status such as `Configured JMAP endpoint` rather than the
  endpoint itself.

### Extension readiness

The current `needs_setup` boolean should evolve into something like a setup
state summary that can account for more than missing secrets:

```text
setup_state = "ready" | "needs_input" | "invalid" | "error"
```

The frontend can still show a simple `Setup` button, but the backend should no
longer define readiness solely as "all required secrets exist".

## Extension Setup Schema Changes

### Add typed setup fields for WASM tools

Current tool setup schema:

```json
{
  "setup": {
    "required_secrets": [
      { "name": "google_oauth_client_id", "prompt": "..." }
    ]
  }
}
```

Proposed direction:

```json
{
  "setup": {
    "fields": [
      {
        "type": "delegated_endpoint",
        "endpoint_name": "jmap-default",
        "prompt": "JMAP endpoint URL",
        "endpoint_kind": "jmap"
      }
    ]
  }
}
```

And for mixed cases:

```json
{
  "setup": {
    "fields": [
      {
        "type": "delegated_endpoint",
        "endpoint_name": "jmap-default",
        "prompt": "JMAP endpoint URL",
        "endpoint_kind": "jmap"
      },
      {
        "type": "secret",
        "name": "jmap_access_token",
        "prompt": "JMAP access token"
      }
    ]
  }
}
```

This replaces the current secret-only schema assumption with an extensible field
model.

### Keep auth schema separate

`auth` should continue to describe:

- OAuth or manual token acquisition,
- setup instructions,
- validation endpoints for secrets.

Delegated endpoints are not credentials and should not be overloaded into
`auth.setup_url`, `auth.validation_endpoint`, or secret fields.

## New Endpoint Binding Store

### Why a separate store is required

Delegated endpoints are not a good fit for:

- `SecretsStore`, because they are not credentials,
- `tool_capabilities`, because that stores shipped static capability ceilings,
- generic extension provenance metadata, because this is per-user runtime
  configuration.

IronClaw should add a dedicated per-user endpoint binding store.

### Proposed record shape

Suggested record:

```text
AuthorizedEndpointBinding {
  id: uuid,
  user_id: string,
  extension_name: string,
  endpoint_name: string,         // e.g. "jmap-default"
  endpoint_kind: string,         // e.g. "jmap"
  endpoint_url_encrypted: bytes,
  normalized_origin: string,     // optional searchable/indexed projection
  path_prefix: string | null,    // optional internal policy
  methods: list<string>,         // optional internal policy
  validation_state: string,      // ready | invalid | pending
  validated_at: timestamp | null,
  last_error_redacted: string | null,
  created_at: timestamp,
  updated_at: timestamp,
}
```

Notes:

- `endpoint_url_encrypted` should be treated as confidential configuration.
- If host leakage risk is taken seriously, avoid storing the raw URL in
  plaintext settings JSON.
- `normalized_origin` is optional and should exist only if needed for indexing
  or joins; if stored, it should be treated carefully in logs and admin tools.

### Storage API

Add a service interface such as:

```text
EndpointBindingStore {
  get(user_id, extension_name, endpoint_name) -> Option<AuthorizedEndpointBinding>
  put(binding)
  delete(user_id, extension_name, endpoint_name)
  validate(binding) -> ValidationResult
  exists_ready(user_id, extension_name, endpoint_name) -> bool
}
```

This service should be owned by IronClaw, not by the extension.

## WASM Capability and WIT Changes

### Add a delegated endpoint capability

Current HTTP capability:

```text
http {
  allowlist: Vec<EndpointPattern>
  credentials: HashMap<String, CredentialMapping>
}
```

Proposed addition:

```text
authorized_endpoints {
  bindings: Vec<AuthorizedEndpointGrant>
}

AuthorizedEndpointGrant {
  endpoint_name: string,      // e.g. "jmap-default"
  endpoint_kind: string,      // e.g. "jmap"
  methods: Vec<string>,       // optional ceiling
  path_prefixes: Vec<string>, // optional ceiling
}
```

This lets a tool declare:

- "I may use `jmap-default`",
- without naming the actual host.

### Add a new WIT host call

Current host call:

```text
http-request(method, url, headers-json, body, timeout-ms)
```

Proposed additive host call:

```text
authorized-endpoint-request(
  method,
  endpoint-name,
  path,
  query-json,
  headers-json,
  body,
  timeout-ms,
) -> result<http-response, string>
```

Where:

- `endpoint-name` is an opaque configured endpoint name such as
  `jmap-default`,
- `path` must be a strict relative-path reference. The runtime must reject
  absolute URLs, authority-form references such as `//host/path`, dot-segment
  traversal (`.` or `..`), and encoded traversal forms that would escape the
  stored endpoint or any configured `path_prefixes` before reconstructing the
  final URL,
- `query-json` is a JSON object string or structured map equivalent,
- the extension never supplies the full absolute URL.

Alternative shape:

```text
authorized-endpoint-request(request: delegated-http-request)
```

Either way, the key property is the same:

the guest passes an endpoint identity, not a raw URL.

### Why not overload `http_request`

Overloading `http_request` with magic URL placeholders would be brittle and easy
to misuse. A dedicated host call is clearer because it:

- makes the confidentiality boundary explicit,
- preserves the existing raw URL semantics,
- lets policy and logging branch cleanly,
- avoids accidental leakage from code paths that assume the URL is guest-owned.

## Runtime and Service Changes

### Add an authorized endpoint request path

IronClaw should add a runtime service that handles delegated endpoint requests:

```text
AuthorizedEndpointRequestService {
  resolve_binding(user_id, extension_name, endpoint_name)
  authorize_request(binding, method, path)
  inject_credentials(binding, headers, query, body)
  execute(binding, request_spec)
}
```

### Resolution pipeline

Proposed runtime pipeline:

```text
WASM guest passes endpoint_name + relative request
    -> host checks delegated-endpoint capability grant
    -> host loads binding from EndpointBindingStore
    -> host reconstructs concrete URL internally
    -> host applies internal allowlist and private-IP checks
    -> host injects credentials bound to endpoint identity
    -> host executes request
    -> host redacts endpoint URL from logs/errors
    -> host returns response
```

### Keep server-side request forgery (SSRF) and network hardening

Delegated endpoints should not bypass outbound hardening.

IronClaw should still:

- enforce HTTPS,
- reject internal/private IP resolution,
- reject userinfo in URLs,
- constrain path and method usage,
- rate-limit per execution and globally,
- fail closed if the binding is missing or invalid.

The difference is that these checks move behind endpoint resolution rather than
running against a guest-visible absolute URL.

## Credential Injection Changes

### Today

Credential injection is keyed by `host_patterns`.

That works only when:

- the request host is visible to the extension,
- the extension capabilities file already knows the host,
- the runtime joins credentials to requests by host glob.

### Proposed change

For delegated endpoints, credential authority should be keyed by endpoint
identity rather than by visible host patterns.

Suggested new abstraction:

```text
EndpointCredentialBinding {
  endpoint_name: string,
  secret_name: string,
  location: CredentialLocation,
}
```

This means:

- the extension does not know the actual host,
- credential injection no longer depends on `host_patterns`,
- the host can still inject Authorization headers, query params, or URL path
  placeholders after resolution.

### Compatibility

Keep `CredentialMapping.host_patterns` for raw URL `http_request`.
Add `EndpointCredentialBinding` only for delegated authorized requests.

## Security Model Changes

### Endpoint URL becomes confidential configuration

Today, IronClaw primarily protects credential values.

Under this RFC, the concrete endpoint URL must also be treated as sensitive
runtime configuration in at least three places:

1. logs and tracing,
2. tool errors and network failures,
3. approval and agent-visible event payloads.

### Approval and observability surfaces

When the agent invokes a JMAP tool, IronClaw should surface only:

- extension name,
- endpoint identity such as `jmap-default`,
- method and sanitized relative path summary,
- whether a delegated authorized request occurred.

It should not surface:

- the concrete host,
- the full URL,
- redacted-but-still-identifiable URL fragments.

### Redaction

Add an endpoint-aware redaction layer so reqwest and transport errors cannot
leak the resolved URL back into the tool result or system logs.

Suggested rule:

- raw URL request path: redact secrets as today,
- delegated endpoint path: redact secrets and the resolved endpoint URL.

### Fail-closed rules

Delegated request execution must fail before any network attempt if:

- the binding does not exist,
- the binding is not ready,
- the extension lacks a delegated-endpoint capability grant,
- the reconstructed URL violates internal policy,
- credential injection requirements cannot be satisfied.

## JMAP-Specific Notes

### Minimal first cut

The smallest useful JMAP shape is:

- one configured hidden binding, `jmap-default`,
- one delegated request path to the JMAP session endpoint,
- optional bearer token or other credential injected by the host.

That is enough for "check my email" workflows that operate through the primary
JMAP endpoint.

### Likely follow-up question

JMAP session data may expose additional URLs such as:

- upload,
- download,
- event source.

This RFC does not require phase one to solve all of those. It does require the
design to leave room for either:

- one binding expanding into several host-owned derived endpoints, or
- multiple delegated endpoint bindings grouped under a provider namespace.

## API Sketch

### Example tool capabilities

```json
{
  "version": "0.3.0",
  "wit_version": "0.3.0",
  "authorized_endpoints": {
    "bindings": [
      {
        "endpoint_name": "jmap-default",
        "endpoint_kind": "jmap",
        "methods": ["POST"]
      }
    ]
  },
  "setup": {
    "fields": [
      {
        "type": "delegated_endpoint",
        "endpoint_name": "jmap-default",
        "prompt": "JMAP endpoint URL",
        "endpoint_kind": "jmap"
      }
    ]
  }
}
```

### Example guest-side call

```text
authorized-endpoint-request(
  "POST",
  "jmap-default",
  "/",
  "{}",
  "{\"Content-Type\":\"application/json\"}",
  <body>,
  30000,
)
```

The extension knows only `jmap-default`, not the actual endpoint URL.

## Compatibility and Migration

### Existing extensions

No breaking change is required for existing extensions if IronClaw:

- keeps `http_request`,
- keeps `http.allowlist`,
- keeps `CredentialMapping.host_patterns`,
- treats delegated endpoint requests as a new optional capability path.

### New migrations

Expected backend changes:

- new DB table or equivalent persistence for endpoint bindings,
- new extension setup DTOs,
- new WIT host method and runtime plumbing,
- new redaction coverage and tests,
- updated extension author documentation.

## Alternatives Considered

### 1. Store the endpoint URL as a secret

Rejected.

Reason:

- it blurs the difference between credentials and host-owned endpoint
  configuration,
- it does not solve the raw URL `http_request` problem by itself,
- it invites future misuse where secrets semantics are assumed to apply.

### 2. Keep raw URL requests but hide the URL from the agent only

Rejected.

Reason:

- the requirement is stronger: the extension itself should not see the URL
  either,
- the current WIT contract would still reveal the raw URL to guest code.

### 3. Add the JMAP host to the extension allowlist and accept visibility

Rejected for this RFC.

Reason:

- that is the current model and explicitly does not satisfy the goal.

### 4. Use placeholder URLs in capabilities

Rejected.

Reason:

- placeholder URLs still overload a raw URL API,
- policy, credential injection, and logging become ambiguous,
- a dedicated delegated-endpoint request path is clearer and safer.

## Rollout Plan

### Phase 1: Data model and setup schema

- Add typed setup fields for WASM tools.
- Add endpoint binding persistence and validation service.
- Update extension setup DTOs and UI to support delegated endpoint fields.

### Phase 2: Runtime capability and WIT

- Add delegated endpoint capability schema.
- Add `authorized-endpoint-request` to WIT and runtime wrapper plumbing.
- Add endpoint-aware redaction and audit behaviour.

### Phase 3: JMAP pilot

- Implement the hypothetical JMAP extension against the new delegated request
  path.
- Validate that the extension can operate with no raw endpoint URL visibility.
- Confirm that agent-visible logs and errors do not leak the endpoint.

### Phase 4: Generalization

- Extend the model to other endpoint-sensitive provider tools as needed.
- Decide whether endpoint bindings can be shared across extensions or must stay
  extension-scoped.

## Open Questions

1. Should delegated endpoint bindings be extension-scoped, provider-scoped, or
   globally reusable within a user account?
2. Should endpoint URLs be encrypted at rest using the existing secrets
   envelope, or should IronClaw keep a separate confidential-config store?
3. How should approval surfaces describe delegated requests without leaking
   meaningful origin details?
4. Should delegated endpoint bindings allow multiple named instances per
   extension, or should phase one be single-binding only?
5. For JMAP specifically, do upload/download/event-source URLs need to be part
   of phase one, or can the first delivery cover only the session endpoint?

## Recommendation

Adopt a first-class delegated endpoint model instead of trying to stretch the
current secret-only setup and raw URL HTTP model beyond what they were designed
to do.

The key decision is architectural:

- do not treat this as a small `host_patterns` or `allowlist` tweak,
- do not store the endpoint as just another secret and keep the same WIT call,
- do add a new endpoint-binding store, typed setup fields, a delegated request
  host API, and endpoint-aware redaction.

That is the smallest design that honestly satisfies the requirement that the
configured JMAP endpoint be usable by the host on the extension's behalf while
remaining hidden from both the extension and the agent.
