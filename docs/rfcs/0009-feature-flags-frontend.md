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
enabled, and lets the browser gate rendering and behaviour accordingly,
without introducing a third-party feature-flag service or a new
persistence layer.

## Problem

### No runtime control over front-end capability exposure

The front-end static assets (`index.html`, `style.css`, `app.js`) are
embedded at compile time via `include_str!` and `include_bytes!`. Once
the binary is built, every tab, button, and control surface the UI
defines is unconditionally available. There is no way for an operator or
developer to:

- disable an experimental tab or feature in a production deployment
  without rebuilding the binary,
- hide an incomplete UI surface behind a gate that can be toggled at
  runtime,
- roll out a new front-end capability progressively across deployments,
  or
- suppress a front-end surface whose backing subsystem is absent from
  `GatewayState` (the browser currently discovers this only through
  `503 Service Unavailable` responses when a handler cannot locate its
  subsystem reference).

### Gateway status is not designed to carry feature metadata

The `/api/gateway/status` endpoint returns operational telemetry
(version, connection counts, uptime, cost tracking, restart
eligibility). The browser already derives one UI decision from this
response: it enables or disables the restart button based on
`restart_enabled`. However, `GatewayStatusResponse` is not designed to
carry an open-ended set of feature gates, and adding ad-hoc booleans to
this struct for every new feature would conflate operational telemetry
with capability negotiation.

### Existing settings are user-scoped preferences, not deployment flags

The database-backed settings system (`/api/settings`) stores per-user
key–value preferences. These settings express user intent (for example,
preferred log level or display options) and are persisted in the
`settings` table per `user_id`. Feature flags are a different concern:
they express deployment-level or operator-level decisions about which
capabilities are available, and should not be conflated with user
preferences.

## Current state

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

### GatewayState subsystem optionality

`GatewayState` holds optional `Arc` references to runtime subsystems.
Handlers degrade by returning `503 Service Unavailable` when the
required subsystem is `None`. The browser does not learn which
subsystems are absent until the user navigates to a panel that calls
a failing endpoint. This produces a poor experience: the user sees an
enabled tab, clicks it, and receives an error.

### Compile-time Rust feature gates

The Rust build uses Cargo feature flags (`libsql`, `postgres`, and so
on) for backend-selection purposes. These are compile-time choices that
affect which database driver is linked, not runtime front-end feature
toggles.

## Goals and non-goals

- Goals:
  - Define a mechanism for the backend to declare a set of named
    feature flags with boolean values.
  - Expose those flags to the browser through a dedicated API endpoint.
  - Establish a front-end consumption pattern so `app.js` can gate UI
    rendering and behaviour on flag values.
  - Support operator-level flag configuration through environment
    variables.
  - Support runtime flag updates through the existing settings API
    without requiring a restart.
  - Allow `GatewayState` subsystem availability to contribute to flag
    resolution so that flags can reflect actual backend capability.
- Non-goals:
  - Multi-user or per-user feature targeting. Flags apply to the
    deployment, not to individual users.
  - A/B testing, percentage rollouts, or statistical experiment
    infrastructure.
  - A third-party feature-flag service integration (LaunchDarkly,
    Unleash, and the like).
  - Backend-only feature gating (for example, gating agent-loop
    behaviour). This RFC covers only the path from backend
    configuration to browser consumption.
  - Persisting flags in the database as a new table. The existing
    settings infrastructure is reused for operator overrides.

## Proposed design

### 1. Feature-flag registry

Introduce a `FeatureFlagRegistry` struct that holds the resolved set of
flags for the running gateway instance. Each flag is a named boolean:

```rust,no_run
/// A resolved set of feature flags for the current gateway instance.
pub struct FeatureFlagRegistry {
    flags: HashMap<String, bool>,
}
```

The registry is constructed once during gateway startup and updated when
operator overrides change. It is not a per-request computation.

### 2. Flag resolution order

Each flag is resolved through a three-tier precedence chain:

1. **Environment variable** (highest precedence): An environment
   variable of the form `FEATURE_FLAG_<UPPER_SNAKE_NAME>` explicitly
   sets the flag. The value `true` (case-insensitive) enables the flag;
   any other value disables it.
2. **Operator override via settings**: A setting stored in the database
   under the key prefix `feature_flag:` (for example,
   `feature_flag:jobs_tab`) takes effect when no environment variable
   is present for that flag.
3. **Compiled default** (lowest precedence): A static default compiled
   into the binary provides the baseline when neither the environment
   nor the settings store specifies a value.

This precedence mirrors the existing configuration loading strategy
(environment > database settings > defaults) documented in
`src/config/mod.rs`.

### 3. Subsystem-derived flags

Certain flags should reflect whether the backing subsystem is actually
available in `GatewayState`. For example, the `jobs_tab` flag should
resolve to `false` when `GatewayState::job_manager` is `None`,
regardless of the compiled default, unless an explicit operator override
forces it on. The registry supports a fourth resolution input for these
cases:

- **Subsystem availability** (applied after the compiled default but
  before operator and environment overrides): If a flag is annotated
  as subsystem-derived, and the corresponding `GatewayState` field is
  `None`, the flag defaults to `false`. An operator may still force
  the flag to `true` through an environment variable or a settings
  override.

Resolution order summary:

<!-- markdownlint-disable MD013 MD060 -->
| Priority | Source | Example |
|----------|--------|---------|
| 1 (highest) | Environment variable | `FEATURE_FLAG_JOBS_TAB=false` |
| 2 | Operator override in settings | `feature_flag:jobs_tab` = `"true"` |
| 3 | Subsystem availability | `GatewayState::job_manager.is_some()` |
| 4 (lowest) | Compiled default | `true` |
<!-- markdownlint-enable MD013 MD060 -->

_Table 1: Feature-flag resolution precedence._

### 4. Initial flag catalogue

The initial set of flags covers the main UI surfaces that depend on
optional subsystems:

<!-- markdownlint-disable MD013 MD060 -->
| Flag name | Subsystem dependency | Compiled default | Purpose |
|-----------|---------------------|------------------|---------|
| `jobs_tab` | `job_manager` | `true` | Show or hide the Jobs tab |
| `routines_tab` | `routine_engine` | `true` | Show or hide the Routines tab |
| `extensions_tab` | `extension_manager` | `true` | Show or hide the Extensions tab |
| `skills_tab` | `skill_registry` | `true` | Show or hide the Skills tab |
| `memory_tab` | `workspace` | `true` | Show or hide the Memory tab |
| `logs_tab` | `log_broadcaster` | `true` | Show or hide the Logs tab |
<!-- markdownlint-enable MD013 MD060 -->

_Table 2: Initial feature-flag catalogue._

Additional flags (for example, gating an experimental UI component) can
be added by registering a new entry in the compiled defaults map.

### 5. Backend API endpoint

Expose a new authenticated endpoint:

```plaintext
GET /api/features
```

The response is a JSON object mapping flag names to boolean values:

```json
{
  "jobs_tab": true,
  "routines_tab": true,
  "extensions_tab": false,
  "skills_tab": true,
  "memory_tab": true,
  "logs_tab": true
}
```

The handler reads from the in-memory `FeatureFlagRegistry` held in
`GatewayState`, so the response is a cheap map serialization with no
database round-trip on the hot path.

### 6. GatewayState integration

Add a `feature_flags` field to `GatewayState`:

```rust,no_run
pub struct GatewayState {
    // ... existing fields ...
    pub feature_flags: Arc<RwLock<FeatureFlagRegistry>>,
}
```

The field is wrapped in `Arc<RwLock<...>>` so that flag values can be
refreshed at runtime (for example, when an operator writes a new
override through `/api/settings`) without restarting the gateway.

Construction sequence:

1. `GatewayChannel::new()` builds the initial registry from compiled
   defaults and environment variables.
1. Each `with_*` builder method that injects an optional subsystem also
   updates the registry's subsystem-derived inputs.
1. After all builder methods have run and before the server starts,
   `start_server()` loads any operator overrides from the settings
   store and applies them.

### 7. Runtime flag refresh

When an operator writes a setting under the `feature_flag:` prefix
through `PUT /api/settings/{key}`, the settings handler notifies the
feature-flag registry to re-resolve the affected flag. This allows
runtime toggling without a process restart.

The refresh path is:

1. `PUT /api/settings/feature_flag:jobs_tab` with body
   `{ "value": "false" }`.
1. The settings handler persists the value normally.
1. The settings handler checks whether the key starts with
   `feature_flag:` and, if so, calls
   `FeatureFlagRegistry::apply_override()` on the shared registry.
1. Subsequent `GET /api/features` responses reflect the updated value.

No SSE event is emitted for flag changes in this initial design. The
browser re-fetches flags on reconnect and page reload, which is
sufficient for deployment-level toggles that change infrequently. A
future iteration could add an SSE `feature_flags_changed` event if
real-time propagation becomes necessary.

### 8. Front-end consumption

#### 8.1 Fetch flags after authentication

Add a `loadFeatureFlags()` call to the post-authentication sequence in
`app.js`, between the `startGatewayStatusPolling()` call and the data
loading calls:

```javascript
let featureFlags = {};

function loadFeatureFlags() {
    return apiFetch('/api/features').then(function (flags) {
        featureFlags = flags;
        applyFeatureFlags();
    });
}
```

#### 8.2 Apply flags to the DOM

`applyFeatureFlags()` shows or hides tab buttons and panels based on
flag values:

```javascript
function applyFeatureFlags() {
    var tabMappings = {
        jobs_tab: 'jobs',
        routines_tab: 'routines',
        extensions_tab: 'extensions',
        skills_tab: 'skills',
        memory_tab: 'memory',
        logs_tab: 'logs',
    };
    Object.keys(tabMappings).forEach(function (flag) {
        var tabId = tabMappings[flag];
        var enabled = featureFlags[flag] !== false;
        var btn = document.querySelector(
            '[data-tab="' + tabId + '"]'
        );
        var panel = document.getElementById('tab-' + tabId);
        if (btn) btn.style.display = enabled ? '' : 'none';
        if (panel) panel.style.display = enabled ? '' : 'none';
    });
}
```

#### 8.3 Guard feature-specific logic

Code paths that initialize data for a gated feature should check the
flag before making API calls:

```javascript
if (featureFlags.jobs_tab !== false) {
    loadJobs();
}
```

This avoids unnecessary network requests for disabled features and
prevents `503` errors when the backing subsystem is absent.

## Requirements

### Functional requirements

- The backend must expose a `GET /api/features` endpoint returning
  the resolved feature-flag map as a JSON object.
- The front end must fetch and apply feature flags before rendering
  feature-dependent UI surfaces.
- Flags must be resolvable from environment variables, operator
  overrides in the settings store, subsystem availability, and
  compiled defaults, in that precedence order.
- An operator must be able to change a flag value at runtime through
  the existing settings API without restarting the gateway.

### Technical requirements

- The flag registry must be held in `GatewayState` so handlers can
  access it through the standard Axum state extraction pattern.
- The `GET /api/features` handler must not perform a database query
  on every request; it must read from the in-memory registry.
- The flag registry must be safe for concurrent reads from multiple
  handler tasks.
- Flag names must use `snake_case` and must be valid JSON object keys.

## Compatibility and migration

This change is additive. No existing API contract changes.

- The `GET /api/features` endpoint is new; older front-end assets
  that do not call it continue to work with all features visible.
- The settings key prefix `feature_flag:` is a namespace convention
  within the existing settings table; no schema migration is needed.
- Environment variable names follow the existing `FEATURE_FLAG_`
  prefix convention and do not conflict with any current variables.
- The `GatewayStatusResponse` struct is not modified; operational
  telemetry and feature negotiation remain separate concerns.

## Alternatives considered

### Option A: Extend GatewayStatusResponse

Add flag fields directly to the existing `GatewayStatusResponse`
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
browser would read flags synchronously from `window.__FEATURE_FLAGS__`
at boot.

This eliminates the extra HTTP round-trip but breaks the current
compile-time embedding model. The HTML response would need to be
generated per-request (or cached and invalidated on flag change),
the `Content-Type` and caching headers would need adjustment, and the
static-asset serving path would become more complex. The operational
simplicity of the current approach — one binary, no templating — is
worth preserving.

### Option C: Reuse the settings endpoint directly

Store flags as regular settings keys (for example,
`feature_flag:jobs_tab`) and have the front end read all settings at
boot to extract the `feature_flag:` subset.

This avoids a new endpoint but leaks internal setting names to the
front end, forces the browser to filter a potentially large settings
payload, and does not account for subsystem-derived defaults or
environment-variable overrides. The front end would receive raw
operator overrides without the resolution logic that determines the
effective flag value.

<!-- markdownlint-disable MD013 MD060 -->
| Concern | Proposed (dedicated endpoint) | Option A (extend status) | Option B (HTML injection) | Option C (reuse settings) |
|---------|-------------------------------|--------------------------|---------------------------|---------------------------|
| Separation of concerns | Clean | Conflated with telemetry | Clean | Conflated with user preferences |
| Extra HTTP round-trip | One, at boot | None (piggybacks on poll) | None | One, at boot |
| Compile-time embedding | Preserved | Preserved | Broken | Preserved |
| Subsystem-derived defaults | Supported | Possible but awkward | Supported | Not supported |
| Runtime refresh | Via settings API | Requires status struct rebuild | Requires cache invalidation | Partial (no resolution) |
<!-- markdownlint-enable MD013 MD060 -->

_Table 3: Comparison of alternatives._

## Open questions

- Should flag changes emit an SSE event so the browser can react
  without a page reload? The current proposal defers this, but a
  `feature_flags_changed` event could be added if operators need
  near-instant propagation.
- Should the flag registry support non-boolean values (for example,
  string variants or integers) for future use cases such as selecting
  between multiple implementations of a feature? The current proposal
  restricts flags to booleans for simplicity.
- Should the `GET /api/features` response include metadata per flag
  (such as the resolution source or a human-readable description), or
  should it remain a flat boolean map?

## Recommendation

Adopt the proposed design: a dedicated `GET /api/features` endpoint
backed by an in-memory `FeatureFlagRegistry` in `GatewayState`, with
flags resolved from environment variables, operator overrides, subsystem
availability, and compiled defaults. This approach preserves the
existing compile-time asset model, separates feature negotiation from
operational telemetry, reuses the settings infrastructure for operator
overrides, and gives the front end a clean, single-purpose API to fetch
the effective flag state at boot.
