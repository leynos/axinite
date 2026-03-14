# RFC 0009: Feature flags for the web front end

## Preamble

- **RFC number:** 0009
- **Status:** Proposed
- **Created:** 2026-03-14

## Summary

Axinite's web front end currently has no mechanism for the backend to
communicate feature availability to the browser. Every capability the
UI exposes is unconditionally rendered, and toggling experimental or
incomplete surfaces requires a code change, a rebuild, and a
redeployment. This RFC proposes a lightweight feature-flag delivery
mechanism that lets the backend declare which front-end capabilities
are enabled, lets the browser gate rendering and behaviour accordingly,
and fits into the existing configuration model (environment variables,
TOML overlay, and compiled defaults).

## Problem

### No runtime control over front-end capability exposure

The front-end static assets (`index.html`, `style.css`, `app.js`) are
embedded at compile time via `include_str!` and `include_bytes!`. Once
the binary is built, every tab, button, and control surface the UI
defines is unconditionally available. There is no way for an operator
to:

- disable an experimental feature in a production deployment without
  rebuilding the binary,
- hide an incomplete UI surface behind a gate that can be toggled at
  runtime, or
- roll out a new front-end capability progressively across
  deployments.

### Gateway status is not designed to carry feature metadata

The `/api/gateway/status` endpoint returns operational telemetry
(version, connection counts, uptime, cost tracking, restart
eligibility). The browser already derives one UI decision from this
response: it enables or disables the restart button based on
`restart_enabled`. However, `GatewayStatusResponse` is not designed to
carry an open-ended set of feature gates, and adding ad-hoc booleans
to this struct for every new feature would conflate operational
telemetry with capability negotiation.

### Existing settings are user-scoped preferences, not deployment flags

The database-backed settings system (`/api/settings`) stores per-user
key-value preferences. Feature flags are a different concern: they
express deployment-level decisions about which capabilities are
available, and should not be conflated with user preferences.

## Current state

### Configuration model

Axinite resolves configuration through a layered precedence chain
(see `src/config/mod.rs`):

1. Environment variables (highest precedence).
1. TOML config file overlay (`~/.ironclaw/config.toml`).
1. Database settings.
1. Compiled defaults (lowest precedence).

The TOML file deserializes directly into the `Settings` struct via
`toml::from_str()`. The `merge_from()` method applies only values that
differ from defaults, so the TOML overlay does not clobber
previously-set database values.

For list-valued configuration, the codebase already uses a
comma-separated environment variable pattern (see
`src/config/channels.rs`):

```rust,no_run
let items = optional_env("SIGNAL_ALLOW_FROM")?
    .map(|s| {
        s.split(',')
            .map(|e| e.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    })
    .unwrap_or_default();
```

The same values can be expressed as TOML arrays in the config file.

### Front-end boot sequence

After successful authentication, `app.js` performs the following
initialization:

1. Store the token in `sessionStorage`.
1. Connect to the chat SSE stream (`/api/chat/events`).
1. Connect to the logs SSE stream (`/api/logs/events`).
1. Start gateway-status polling (`/api/gateway/status`, 30-second
   interval).
1. Check Trusted Execution Environment (TEE) attestation state.
1. Load threads, memory tree, and jobs.

No dedicated configuration or feature-flag fetch occurs during this
sequence. The browser renders all tabs and controls unconditionally.

## Goals and non-goals

- Goals:
  - Define a mechanism for the backend to declare a set of named
    feature flags as enabled.
  - Expose those flags to the browser through a dedicated API
    endpoint.
  - Accept flags through the existing configuration inputs: a
    comma-separated environment variable and a TOML array.
  - Establish a front-end consumption pattern so `app.js` can gate
    UI rendering and behaviour on flag values.
- Non-goals:
  - Multi-user or per-user feature targeting. Flags apply to the
    deployment, not to individual users.
  - A/B testing, percentage rollouts, or statistical experiment
    infrastructure.
  - A third-party feature-flag service integration (LaunchDarkly,
    Unleash, and the like).
  - Defining which specific flags exist or what they control.
    Specific flag names are a product concern; this RFC defines the
    delivery mechanism.
  - Backend-only feature gating (for example, gating agent-loop
    behaviour). This RFC covers only the path from backend
    configuration to browser consumption.

## Proposed design

### 1. Configuration inputs

Operators declare which feature flags are enabled through two
complementary configuration inputs, following the established
patterns in `src/config/channels.rs` and `src/config/helpers.rs`.

#### Environment variable

A single comma-separated environment variable lists enabled flags:

```plaintext
FEATURE_FLAGS=experimental_chat_ui,new_memory_search,dark_mode
```

Parsing follows the existing pattern: split on `,`, trim whitespace,
discard empty segments.

#### TOML config file

The same flags can be declared as an array in the TOML config file
under a `[gateway]` section:

```toml
[gateway]
feature_flags = ["experimental_chat_ui", "new_memory_search", "dark_mode"]
```

#### Precedence

The environment variable takes precedence over the TOML array, which
takes precedence over the compiled default (an empty list). This
matches the standard configuration resolution order. When the
environment variable is set, it replaces the TOML value entirely
rather than merging with it; this is consistent with how other
list-valued configuration behaves in the codebase.

### 2. Data shape

Feature flags are modelled as a set of opaque string names. A flag
is enabled if and only if it appears in the resolved set. This is
deliberately minimal:

- No boolean values: presence means enabled, absence means disabled.
- No metadata, descriptions, or categories in the runtime
  representation.
- No schema validation against a compiled catalogue. Unknown flag
  names are accepted and forwarded to the front end, which ignores
  names it does not recognise.

#### Rust representation

```rust,no_run
/// The resolved set of enabled feature flags for this gateway
/// instance.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct FeatureFlags {
    flags: Vec<String>,
}
```

The struct is constructed once at gateway startup from the resolved
configuration.

### 3. GatewayState integration

Add a `feature_flags` field to `GatewayState`:

```rust,no_run
pub struct GatewayState {
    // ... existing fields ...
    pub feature_flags: FeatureFlags,
}
```

The field is a plain value rather than an `Arc<RwLock<...>>` because
flags are immutable for the lifetime of the process. An operator who
changes the environment variable or TOML file restarts the gateway
to pick up the change, which is consistent with how other
configuration is applied.

Construction sequence:

1. `GatewayConfig::resolve()` in `src/config/channels.rs` reads
   `FEATURE_FLAGS` from the environment (or falls back to the TOML
   overlay value, or falls back to an empty list).
1. The resolved list is stored in `GatewayConfig`.
1. `GatewayChannel::new()` copies the list into `FeatureFlags` and
   sets it on the initial `GatewayState`.

### 4. API endpoint

Expose a new authenticated endpoint:

```plaintext
GET /api/features
```

The response is a JSON object with a single `flags` field containing
the list of enabled flag names:

```json
{
  "flags": [
    "experimental_chat_ui",
    "new_memory_search",
    "dark_mode"
  ]
}
```

The handler reads from the `FeatureFlags` value in `GatewayState` and
serializes it directly. No database query is involved.

#### Why a list, not an object

The response uses an array of strings rather than an object mapping
flag names to booleans. Since absence from the list means "disabled",
a boolean map would always map every present key to `true`, which
adds no information. The array representation is more compact and
avoids the temptation to introduce tri-state semantics (`true` /
`false` / absent) that the configuration model does not support.

### 5. Front-end consumption

#### 5.1 Fetch flags after authentication

Add a `loadFeatureFlags()` call to the post-authentication sequence
in `app.js`, between the `startGatewayStatusPolling()` call and the
data-loading calls:

```javascript
let featureFlags = new Set();

function loadFeatureFlags() {
    return apiFetch('/api/features').then(function (data) {
        featureFlags = new Set(data.flags || []);
    });
}
```

A `Set` gives constant-time membership tests.

#### 5.2 Guard feature-dependent rendering

Code paths that depend on a feature flag check the set:

```javascript
if (featureFlags.has('experimental_chat_ui')) {
    // Render experimental chat controls.
}
```

The front end ignores unknown flags and renders its default surfaces
when no flags are present. This means a deployment that sets no
`FEATURE_FLAGS` variable behaves identically to the current UI.

## Requirements

### Functional requirements

- The backend must expose a `GET /api/features` endpoint returning
  the resolved set of enabled feature-flag names.
- The front end must fetch and apply feature flags before rendering
  feature-dependent UI surfaces.
- Flags must be configurable through a comma-separated environment
  variable (`FEATURE_FLAGS`) and a TOML array
  (`[gateway] feature_flags`).
- When no flags are configured, the endpoint must return an empty
  list and the front end must render its default state.

### Technical requirements

- `FeatureFlags` must be held in `GatewayState` so handlers can
  access it through the standard Axum state extraction pattern.
- The `GET /api/features` handler must not perform a database query;
  it must read from the in-memory value.
- Flag names must consist of lowercase ASCII letters, digits, and
  underscores. The parser should reject or silently discard names
  that do not match this pattern.

## Compatibility and migration

This change is additive. No existing API contracts change.

- The `GET /api/features` endpoint is new; older front-end assets
  that do not call it continue to work with all features visible.
- The `FEATURE_FLAGS` environment variable is new and does not
  conflict with any current variables.
- The `GatewayConfig` struct gains a new field with a default of an
  empty list, so existing configuration files and deployments are
  unaffected.
- The `GatewayStatusResponse` struct is not modified; operational
  telemetry and feature negotiation remain separate concerns.

## Alternatives considered

### Option A: Extend GatewayStatusResponse

Add a `feature_flags` field directly to the existing
`GatewayStatusResponse` returned by `GET /api/gateway/status`.

This is the lowest-effort option but conflates operational telemetry
with capability negotiation. The status endpoint is polled every
30 seconds for connection and cost data; adding feature flags to it
means the browser processes flag data on every poll cycle even though
flags change rarely. It also makes the status response contract
increasingly unwieldy as flags accumulate.

### Option B: Inject flags into HTML at serve time

Replace `include_str!("index.html")` with a template that injects a
`<script>` block containing the flag set as a global variable. The
browser would read flags synchronously from
`window.__FEATURE_FLAGS__` at boot.

This eliminates the extra HTTP round-trip but breaks the current
compile-time embedding model. The HTML response would need to be
generated per-request (or cached and invalidated on flag change),
the `Content-Type` and caching headers would need adjustment, and the
static-asset serving path would become more complex. The operational
simplicity of the current approach — one binary, no templating — is
worth preserving.

### Option C: Boolean map instead of a string list

Return `{ "experimental_chat_ui": true, "dark_mode": false }` instead
of a list of enabled names.

A boolean map suggests that the backend knows the universe of all
possible flags and can state whether each one is on or off. The
proposed design avoids that: the backend forwards whatever the
operator configured, and the front end decides which names it cares
about. A string list is a better fit for this open-ended model.

<!-- markdownlint-disable MD013 MD060 -->
| Concern | Proposed (dedicated endpoint, string list) | Option A (extend status) | Option B (HTML injection) | Option C (boolean map) |
|---------|----------------------------------------------|--------------------------|---------------------------|------------------------|
| Separation of concerns | Clean | Conflated with telemetry | Clean | Clean |
| Extra HTTP round-trip | One, at boot | None (piggybacks on poll) | None | One, at boot |
| Compile-time embedding | Preserved | Preserved | Broken | Preserved |
| Open-ended flag model | Supported | Awkward | Supported | Implies closed universe |
| Configuration complexity | Minimal (one env var, one TOML key) | Minimal | Moderate (templating) | Minimal |
<!-- markdownlint-enable MD013 MD060 -->

_Table 1: Comparison of alternatives._

## Open questions

- Should the `GET /api/features` response include the gateway
  version alongside the flag list so the front end can correlate
  flag availability with the host build?
- Should the endpoint also accept `POST` to allow runtime flag
  toggling without a restart, or is restart-to-apply sufficient for
  deployment-level flags?
- Should the front end re-fetch flags periodically or on SSE
  reconnect, or is a single fetch at boot sufficient?

## Recommendation

Adopt the proposed design: a dedicated `GET /api/features` endpoint
backed by an immutable `FeatureFlags` value in `GatewayState`, with
flags sourced from a comma-separated `FEATURE_FLAGS` environment
variable or a `[gateway] feature_flags` TOML array. This approach
preserves the existing compile-time asset model, separates feature
negotiation from operational telemetry, reuses the established
configuration patterns, and gives the front end a clean,
single-purpose API to fetch the enabled flag set at boot.
