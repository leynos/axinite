# RFC 0009: Feature flags for the web front end

## Preamble

- **RFC number:** 0009
- **Status:** Proposed
- **Created:** 2026-03-14

## Summary

Axinite's web front end currently has no mechanism for the backend to
communicate feature availability to the browser. Every capability the UI
exposes is unconditionally rendered, and toggling experimental or
incomplete surfaces requires a code change, a rebuild, and a
redeployment. This RFC proposes a lightweight feature-flag delivery
mechanism that lets the backend declare which front-end capabilities are
enabled, lets the browser gate rendering and behaviour accordingly, and
supports runtime toggles through operator overrides without requiring a
process restart.

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

### Front-end boot sequence

After successful authentication, `app.js` performs the following
initialization:

1. Store the token in `sessionStorage`.
1. Connect to the chat Server-Sent Events (SSE) stream
   (`/api/chat/events`).
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
    feature flags with boolean values.
  - Expose those flags to the browser through a dedicated API
    endpoint.
  - Accept flags through per-flag environment variables and operator
    overrides via the existing settings API.
  - Support runtime flag updates through the settings API without
    requiring a gateway restart.
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

Operators declare individual feature flags through per-flag
configuration inputs, following the established precedence pattern in
`src/config/mod.rs`.

#### Per-flag environment variables

Each flag can be set through an environment variable of the form
`FEATURE_FLAG_<UPPER_SNAKE_NAME>`:

```plaintext
FEATURE_FLAG_EXPERIMENTAL_CHAT_UI=true
FEATURE_FLAG_NEW_MEMORY_SEARCH=false
```

The value `true` (case-insensitive) enables the flag; any other value
(including `false`, `0`, or an empty string) disables it. Unset
variables fall through to the next precedence layer.

#### Operator overrides via settings API

Operators can set flags at runtime through the existing
`/api/settings` endpoint using keys prefixed with `feature_flag:`:

```plaintext
PUT /api/settings/feature_flag:experimental_chat_ui
{ "value": "true" }
```

These overrides are persisted in the database `settings` table and
take effect immediately without requiring a gateway restart. The
settings handler notifies the feature-flag registry to re-resolve the
affected flag.

#### Subsystem availability defaults

Certain flags may default based on whether a backing subsystem is
available in `GatewayState`. For example, a flag controlling a jobs UI
surface might default to `false` when `GatewayState::job_manager` is
`None`. Subsystem defaults are applied when neither an environment
variable nor an operator override is present.

#### Compiled defaults

When no environment variable, operator override, or subsystem default
is present, the flag resolves to a compiled default value (typically
`false` for new experimental features).

#### Precedence

Per-flag resolution follows this order:

1. **Environment variable** (highest precedence): `FEATURE_FLAG_<NAME>`
1. **Operator override**: `feature_flag:<name>` in the settings table
1. **Subsystem availability**: Derived from `GatewayState` field
   presence
1. **Compiled default** (lowest precedence): Hardcoded fallback value

When an operator writes a settings override, the feature-flag registry
re-resolves that flag and subsequent `GET /api/features` requests
reflect the updated value immediately.

### 2. Data shape

Feature flags are modelled as a mutable in-memory registry that
resolves each flag to a boolean value on demand. The registry is held
in `GatewayState` and can be updated at runtime when operator
overrides change.

#### Rust representation

```rust,no_run
/// A mutable registry of resolved feature flags for the current
/// gateway instance.
pub struct FeatureFlagRegistry {
    /// Resolved flag states: name -> enabled
    flags: Arc<RwLock<HashMap<String, bool>>>,
}
```

The registry is constructed once at gateway startup with compiled
defaults and subsystem-derived values, then updated when operator
overrides are written through the settings API.

### 3. GatewayState integration

Add a `feature_flags` field to `GatewayState`:

```rust,no_run
pub struct GatewayState {
    // ... existing fields ...
    pub feature_flags: Arc<FeatureFlagRegistry>,
}
```

The field is wrapped in `Arc` so handlers can share the registry. The
inner `RwLock<HashMap<...>>` allows runtime mutation when operator
overrides change.

Construction sequence:

1. `GatewayConfig::resolve()` or a dedicated initialization function
   reads per-flag environment variables (checking for names matching
   `FEATURE_FLAG_<NAME>`).
1. `GatewayChannel::new()` constructs the initial
   `FeatureFlagRegistry` with compiled defaults and environment
   variable overrides.
1. After subsystems are wired through `with_*` builder methods, the
   registry applies subsystem-derived defaults for relevant flags.
1. The settings handler is extended to detect writes to keys prefixed
   with `feature_flag:` and calls
   `FeatureFlagRegistry::apply_override()` to update the affected
   flag.

### 4. API endpoint

Expose a new authenticated endpoint:

```plaintext
GET /api/features
```

The response is a JSON object mapping flag names to boolean values:

```json
{
  "experimental_chat_ui": true,
  "new_memory_search": false,
  "dark_mode": false
}
```

The handler reads from the `FeatureFlagRegistry` in `GatewayState` and
serializes the resolved flag map. No database query is involved on the
hot path; the registry holds pre-resolved values that are updated only
when operator overrides change.

#### Why a boolean map

The response uses a flat object mapping names to booleans rather than
an array of enabled names. This makes the contract explicit: each flag
has a defined state (`true` or `false`), and the front end does not
need to infer that absence means disabled. The boolean map also
simplifies future extensions such as tri-state flags or metadata
fields.

### 5. Front-end consumption

#### 5.1 Fetch flags after authentication

Add a `loadFeatureFlags()` call to the post-authentication sequence
in `app.js`, between the `startGatewayStatusPolling()` call and the
data-loading calls:

```javascript
let featureFlags = {};

function loadFeatureFlags() {
    return apiFetch('/api/features').then(function (data) {
        featureFlags = data;
    });
}
```

A plain object suffices since flag lookups are infrequent.

#### 5.2 Guard feature-dependent rendering

Code paths that depend on a feature flag check the map:

```javascript
if (featureFlags.experimental_chat_ui) {
    // Render experimental chat controls.
}
```

The front end ignores unknown flags and renders its default surfaces
when flags are absent from the map. This means a deployment that sets
no feature flags behaves identically to the current UI.

## Requirements

### Functional requirements

- The backend must expose a `GET /api/features` endpoint returning
  the resolved feature-flag map as a JSON object.
- The front end must fetch and apply feature flags before rendering
  feature-dependent UI surfaces.
- Flags must be configurable through per-flag environment variables
  (`FEATURE_FLAG_<NAME>`) and runtime operator overrides via
  `/api/settings/feature_flag:<name>`.
- When an operator writes a settings override, the flag must be
  re-resolved immediately without requiring a gateway restart.
- When no flags are configured, the endpoint must return an empty
  object and the front end must render its default state.

### Technical requirements

- `FeatureFlagRegistry` must be held in `GatewayState` so handlers
  can access it through the standard Axum state extraction pattern.
- The `GET /api/features` handler must not perform a database query;
  it must read from the in-memory registry.
- The registry must be safe for concurrent reads and writes from
  multiple handler tasks.
- Flag names must consist of lowercase ASCII letters, digits, and
  underscores. The parser must silently discard names that do not
  match this pattern and log a warning with the invalid name.

## Compatibility and migration

This change is additive. No existing API contracts change.

- The `GET /api/features` endpoint is new; older front-end assets
  that do not call it continue to work with all features visible.
- Per-flag environment variables follow the `FEATURE_FLAG_` prefix
  convention and do not conflict with any current variables.
- The settings key prefix `feature_flag:` is a namespace convention
  within the existing settings table; no schema migration is needed.
- The `GatewayConfig` struct gains initialization logic for
  per-flag environment variable parsing, with a default of no flags
  enabled.
- The `GatewayStatusResponse` struct is not modified; operational
  telemetry and feature negotiation remain separate concerns.

## Alternatives considered

### Option A: Extend GatewayStatusResponse

Add feature flags directly to the existing `GatewayStatusResponse`
returned by `GET /api/gateway/status`.

This is the lowest-effort option but conflates operational telemetry
with capability negotiation. The status endpoint is polled every
30 seconds for connection and cost data; adding feature flags to it
means the browser processes flag data on every poll cycle even though
flags change rarely. It also makes the status response contract
increasingly unwieldy as flags accumulate.

### Option B: Inject flags into HTML at serve time

Replace `include_str!("index.html")` with a template that injects a
`<script>` block containing the flag map as a global variable. The
browser would read flags synchronously from
`window.__FEATURE_FLAGS__` at boot.

This eliminates the extra HTTP round-trip but breaks the current
compile-time embedding model. The HTML response would need to be
generated per-request (or cached and invalidated on flag change),
the `Content-Type` and caching headers would need adjustment, and the
static-asset serving path would become more complex. The operational
simplicity of the current approach — one binary, no templating — is
worth preserving.

### Option C: Comma-separated list instead of per-flag variables

Accept a single `FEATURE_FLAGS=a,b,c` environment variable and return
a list of enabled flag names instead of a boolean map.

A list-based model is simpler for initial configuration but does not
support per-flag environment variables, runtime overrides, or
subsystem-derived defaults. The proposed per-flag model gives
operators finer control and integrates naturally with the existing
settings API.

<!-- markdownlint-disable MD013 MD060 -->
| Concern | Proposed (per-flag, boolean map) | Option A (extend status) | Option B (HTML injection) | Option C (list-based) |
|---------|----------------------------------|--------------------------|---------------------------|-----------------------|
| Separation of concerns | Clean | Conflated with telemetry | Clean | Clean |
| Extra HTTP round-trip | One, at boot | None (piggybacks on poll) | None | One, at boot |
| Compile-time embedding | Preserved | Preserved | Broken | Preserved |
| Runtime overrides | Supported | Awkward | Awkward | Not supported |
| Per-flag control | Supported | Supported | Supported | Not supported |
| Configuration complexity | Moderate (one env var per flag) | Minimal | Moderate (templating) | Minimal (one env var total) |
<!-- markdownlint-enable MD013 MD060 -->

_Table 1: Comparison of alternatives._

## Open questions

- Should the `GET /api/features` response include the gateway
  version alongside the flag map so the front end can correlate
  flag availability with the host build?
- Should flag changes emit an SSE event so the browser can react
  without a page reload? The current proposal defers this, but a
  `feature_flags_changed` event could be added if operators need
  near-instant propagation.
- Should the front end re-fetch flags periodically or on SSE
  reconnect, or is a single fetch at boot sufficient given that
  operator overrides update the registry immediately?

## Recommendation

Adopt the proposed design: a dedicated `GET /api/features` endpoint
backed by a mutable `FeatureFlagRegistry` in `GatewayState`, with
flags sourced from per-flag environment variables, runtime operator
overrides via the settings API, subsystem availability defaults, and
compiled defaults. This approach preserves the existing compile-time
asset model, separates feature negotiation from operational
telemetry, reuses the established settings infrastructure for runtime
overrides, and gives the front end a clean, single-purpose API to
fetch the effective flag state at boot and on reconnect.
