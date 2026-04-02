# SolidJS PWA gap analysis

## 1. Summary

This document compares the current Rust-hosted browser gateway in `../axinite`
with the SolidJS progressive web application (PWA) prototype in
`../axinite-mockup`. The goal is not to critique styling or implementation
taste. The goal is to identify every place where the PWA currently breaks
contract with the existing web front end in one of three ways:

- it expects a data shape the real gateway does not provide,
- it drops or changes information the current UI presents to the user, or
- it omits runtime surfaces that the current browser contract treats as part of
  the product.

The result is a migration-focused gap analysis that the SolidJS front-end
developer can work through directly.

## 2. Method and comparison basis

This analysis is based on code inspection of:

- the current gateway UI and browser-facing API in
  `src/channels/web/static/app.js`,
  `src/channels/web/types.rs`,
  `src/channels/web/handlers/*.rs`,
  `src/channels/web/log_layer.rs`, and `docs/front-end-architecture.md`
- the SolidJS PWA in
  `../axinite-mockup/axinite/src/components/*.tsx`,
  `../axinite-mockup/axinite/src/lib/api/*.ts`,
  `../axinite-mockup/axinite/src/lib/api/contracts.ts`,
  `../axinite-mockup/axinite/src/lib/feature-flags/*.ts*`, and
  `../axinite-mockup/axinite/src/app/router.tsx`
- the Bun mock backend in
  `../axinite-mockup/mock-backend/src/server.ts` and
  `../axinite-mockup/mock-backend/src/state.ts`

This is therefore a comparison of the shipped browser contract against the
current PWA implementation, not against the PWA architecture aspirations.

## 3. Executive summary

The PWA is not merely incomplete. It currently has several hard contract
breaks that would prevent direct adoption against the real gateway.

Table 1. Highest-priority blockers.

| ID | Severity | Area | Gap |
| --- | --- | --- | --- |
| G1 | Critical | Auth and transport | The PWA does not implement gateway bearer-token authentication or query-token handling for SSE. |
| G2 | Critical | Feature flags | The PWA assumes `GET /api/features`, but the real gateway does not implement that endpoint yet. |
| G3 | Critical | Logs payload shape | The PWA expects `LogEntry.source`, while the real gateway emits `LogEntry.target`. |
| G4 | Critical | Job prompt request shape | The PWA sends `{ prompt }`, but the real gateway expects `{ content, done? }`. |
| G5 | High | Surface area | The PWA route set omits shipped browser surfaces such as the Logs tab, settings flows, pairing flows, TEE attestation, restart affordances, and project browser links. |
| G6 | High | Chat events and media | The PWA consumes only a subset of the real chat SSE stream and does not surface generated images, auth cards, or job-start cards. |
| G7 | High | Detail fidelity | Jobs, routines, extensions, and skills screens flatten or drop information that the current UI already exposes. |

The rest of this document expands those gaps into actionable work items.

## 4. Cross-cutting contract breaks

### 4.1. Authentication model is missing from the PWA

- **Current gateway contract**
  - All protected routes require `Authorization: Bearer <token>`.
  - Server-Sent Events (SSE) and WebSocket endpoints support query-string
    tokens because browser APIs cannot set custom headers.
  - The current browser boot flow explicitly authenticates, stores the token,
    and appends it to `/api/chat/events` and `/api/logs/events`.
- **PWA state**
  - `axinite/src/lib/api/client.ts` sends unauthenticated `fetch()` requests.
  - `createEventStream()` opens raw `EventSource(url)` without query-token
    support.
  - `app-shell.tsx` and the route components assume the runtime is already
    reachable with no auth handshake.
- **Impact**
  - The PWA cannot talk to the real gateway outside the mock backend.
  - Every route is effectively wired to the wrong transport assumptions.
- **Required action**
  - Add the same auth boot flow the current browser uses, or replace it with a
    new gateway-auth scheme that the Rust side implements first.
  - Thread bearer tokens through normal `fetch()` requests.
  - Thread query-string tokens through SSE endpoints and any retained WebSocket
    endpoints.

### 4.2. The PWA assumes a feature-flags API that does not exist yet

- **Current gateway contract**
  - The current UI has no `/api/features` fetch path.
  - RFC 0009 proposes `GET /api/features`, but it is not implemented in the
    gateway today.
- **PWA state**
  - `fetchRuntimeFeatureFlags()` calls `/api/features`.
  - Route visibility and the logs panel depend on runtime feature-flag
    resolution.
- **Impact**
  - Against the real gateway, the PWA silently falls back to local defaults and
    therefore hides the fact that it is depending on an unimplemented runtime
    contract.
  - This masks integration risk instead of exposing it.
- **Required action**
  - Either implement RFC 0009 before adopting the PWA, or make the PWA operate
    correctly without `/api/features` until that RFC lands.
  - Do not treat the mock backend endpoint as evidence that the contract
    already exists in Axinite proper.

### 4.3. Several feature flags are defined but not actually honoured

- **Current PWA state**
  - The registry defines `action_memory_edit`, `action_job_restart`,
    `action_routine_trigger`, `action_extension_install`,
    `action_skill_install`, and `surface_tee_attestation`.
  - The actual components use route visibility and `panel_logs`, but they do
    not gate the corresponding actions consistently.
  - Examples:
    - `MemoryPreview` always shows edit/save controls.
    - `JobsPreview` always renders restart and cancel buttons, merely disabling
      some by job state.
    - `RoutinesPreview` always renders run, enable/disable, and delete
      actions.
    - `ExtensionsPreview` and `SkillsPreview` always render install and remove
      affordances.
- **Impact**
  - The front end advertises a progressive-rollout contract that the screens do
    not actually obey.
- **Required action**
  - Either remove these flags until the UI honours them, or wire the buttons
    and panels to the flag registry consistently.

### 4.4. The route surface is smaller than the shipped browser product

- **Current shipped UI**
  - Top-level tabs include Chat, Memory, Jobs, Routines, Extensions, Skills,
    and Logs.
  - The current browser also exposes settings, pairing approval, TEE
    attestation, restart affordances, and project-file browsing from job
    detail.
- **PWA state**
  - Route order is only Chat, Memory, Jobs, Routines, Extensions, and Skills.
  - Logs are downgraded from a top-level workspace tab to a dialog.
  - There is no settings UI.
  - There is no pairing UI.
  - There is no TEE attestation surface.
  - There is no restart affordance.
  - There is no project browser link or equivalent workspace browser surface.
- **Impact**
  - The PWA is presenting a smaller product than the one Axinite currently
    ships.
- **Required action**
  - Decide which of these surfaces are intentionally being removed and record
    that explicitly.
  - For the remainder, add parity tasks rather than letting them disappear by
    omission.

## 5. API and payload-shape gaps

### 5.1. Logs payload mismatch: `source` versus `target`

- **Current gateway contract**
  - `src/channels/web/log_layer.rs` emits log entries with `level`, `target`,
    `message`, and `timestamp`.
  - `logs_events_handler()` replays those entries directly over SSE.
- **PWA state**
  - `contracts.ts` defines `LogEntry` as `id`, `level`, `timestamp`,
    `message`, and `source`.
  - `LogsDialog` renders `[${entry.source}]`.
  - The mock backend publishes `source`, so the mismatch is hidden during
    preview.
- **Impact**
  - Against the real gateway, the logs dialog would show missing or incorrect
    provenance.
- **Required action**
  - Align the PWA log type with the real gateway payload.
  - Decide whether the browser wants `target`, a renamed `source`, or both, and
    then change the Rust side and the mock side together.

### 5.2. Job prompt request body is wrong

- **Current gateway contract**
  - `POST /api/jobs/{id}/prompt` expects `{"content": "...", "done": false}`.
  - `done` is meaningful for Claude Code prompt queues.
- **PWA state**
  - `contracts.ts` defines `JobPromptRequest` as `{ prompt: string }`.
  - `jobs.ts` posts that payload.
  - The mock backend imports the same wrong type and therefore reinforces the
    mismatch.
- **Impact**
  - The PWA cannot send job prompts to the real gateway successfully.
  - The PWA has no UI path for the real `done` signal.
- **Required action**
  - Change the shared contract to `{ content: string; done?: boolean }`.
  - Add the "done" affordance if this route is intended to support the current
    Claude Code activity model.

### 5.3. Chat SSE typing omits real events

- **Current gateway contract**
  - `SseEvent` includes `job_started`, `job_message`, `job_tool_use`,
    `job_tool_result`, `job_status`, `job_result`, `image_generated`,
    `approval_needed`, `auth_required`, `auth_completed`, `extension_status`,
    and others.
- **PWA state**
  - `ChatSseEvent` includes only the general chat events plus auth and
    extension status.
  - It omits `job_started`, the whole `job_*` event family, and
    `image_generated`.
  - `connectChatEvents()` therefore never registers listeners for those events.
- **Impact**
  - The PWA cannot render several currently shipped runtime states even though
    the gateway emits them.
- **Required action**
  - Extend the chat event type and listener registration to the real gateway
    event set, or explicitly split chat-only and job-only streams if that is
    the intended future contract.

### 5.4. Logs-level write shape is permissive in the PWA, but the UX is weaker

- **Current gateway contract**
  - `GET /api/logs/level` and `PUT /api/logs/level` return a minimal shape with
    `level`.
- **PWA state**
  - The PWA can read and write the value, but the rest of the shipped logs UI
    contract is not preserved: there is no level filter for displayed entries,
    no target filter, no pause/resume, no clear action, and no auto-scroll
    control.
- **Impact**
  - The API shape is close, but the user-facing log viewer loses operational
    controls the current UI provides.
- **Required action**
  - Preserve the current operator-facing logs controls, or document that the
    logs surface is being intentionally reduced.

## 6. Shell and global-surface gaps

### 6.1. Gateway status is flattened into a derived label/detail string

- **Current shipped UI**
  - The gateway status widget exposes version, SSE count, WebSocket count,
    uptime, daily cost, actions per hour, per-model token usage, and restart
    availability.
- **PWA state**
  - `fetchGatewayStatus()` reduces the payload to `{ label, detail }`.
  - `AppShell` therefore renders only a simple status pill.
- **Impact**
  - The PWA discards most of the user-visible information the current browser
    makes available to operators.
- **Required action**
  - Preserve the raw status payload in client state.
  - Rebuild an equivalent status popover or panel so model usage, costs,
    uptime, and restart state remain visible.

### 6.2. Restart affordances are missing

- **Current shipped UI**
  - The current shell shows restart availability, restart progress, and restart
    completion behaviour tied to gateway status and tool events.
- **PWA state**
  - No restart button, restart modal, or restart-status handling exists.
- **Impact**
  - A shipped operator capability disappears.
- **Required action**
  - Reintroduce the restart control if that capability remains part of the web
    front end.

### 6.3. TEE attestation surface is missing

- **Current shipped UI**
  - The browser can show a TEE attestation shield and a hover popover with
    digest, TLS fingerprint, report data, VM config, and copy-report action.
- **PWA state**
  - The feature-flag registry contains `surface_tee_attestation`, but there is
    no component implementing it.
- **Impact**
  - The prototype advertises a flag for a surface that does not exist.
- **Required action**
  - Either implement the TEE surface or remove the flag until it is real.

### 6.4. Logs moved from a route to a dialog

- **Current shipped UI**
  - Logs are a first-class top-level tab with filters, pause/resume, clear,
    and continuous stream visibility.
- **PWA state**
  - Logs live in a modal dialog opened from the shell.
- **Impact**
  - The PWA changes the information architecture and makes logs a transient
    secondary surface instead of a first-class workspace.
- **Required action**
  - Decide whether this is an intentional product change.
  - If not, restore route-level logs parity.

## 7. Chat surface gaps

### 7.1. The thread list is less informative

- **Current shipped UI**
  - The thread list shows channel badges for non-gateway threads.
  - It tracks unread counts.
  - It disables the composer for read-only external-channel threads.
  - It derives titles for heartbeat, routine, and channel-backed threads.
- **PWA state**
  - The sidebar shows only title or ID and timestamp.
  - There are no channel badges.
  - There are no unread indicators.
  - There is no read-only composer state.
- **Impact**
  - The user loses thread provenance and read-only state cues.
- **Required action**
  - Preserve `thread_type` and `channel` semantics in the sidebar and composer
    state.

### 7.2. History pagination is missing

- **Current shipped UI**
  - `loadHistory()` supports `limit` and `before`, and the UI can load older
    turns.
- **PWA state**
  - `fetchHistory()` never sends `limit` or `before`.
  - The component has no pagination UI.
- **Impact**
  - Long threads cannot be browsed completely.
- **Required action**
  - Add history pagination support and an affordance for loading older turns.

### 7.3. Image attachment and generated-image flows are missing

- **Current shipped UI**
  - The composer stages images for upload.
  - The chat stream handles `image_generated` and shows generated images.
- **PWA state**
  - The attach button is a stub that only sets a status message.
  - No generated-image rendering exists.
- **Impact**
  - The chat surface loses a shipped media capability.
- **Required action**
  - Implement attachment staging and generated-image rendering, or explicitly
    scope them out of the first migration.

### 7.4. Auth-required flows are not surfaced to the user

- **Current shipped UI**
  - `auth_required` opens either an OAuth card or the extension configure
    modal.
  - `auth_completed` dismisses the auth UI and shows a toast.
- **PWA state**
  - The chat event handler ignores both `auth_required` and `auth_completed`
    for user-facing purposes.
- **Impact**
  - A conversation that triggers extension authentication would appear inert or
    incomplete.
- **Required action**
  - Recreate the auth-card and configure-modal behaviour in the PWA.

### 7.5. Job-start visibility is missing from chat

- **Current shipped UI**
  - `job_started` creates a job card from the chat stream.
- **PWA state**
  - `job_started` is not typed or rendered.
- **Impact**
  - The user loses the bridge from chat activity into sandbox-job activity.
- **Required action**
  - Surface `job_started` in chat or provide an equivalent transition into the
    Jobs screen.

### 7.6. Status semantics are reduced to a generic text line

- **Current shipped UI**
  - Status updates drive input re-enable behaviour and are largely represented
    as inline activity cards.
- **PWA state**
  - `liveStatus` is a generic text note above the composer.
- **Impact**
  - The PWA captures some of the text but not the richer state transitions of
    the shipped UI.
- **Required action**
  - Preserve the status-to-activity rendering model if parity is the goal.

## 8. Memory surface gaps

### 8.1. The PWA collapses the real tree/list model into grouped file buckets

- **Current shipped UI**
  - The browser lazily expands directories using `/api/memory/list?path=...`.
  - The user can browse the actual workspace tree.
- **PWA state**
  - `MemoryPreview` fetches `/api/memory/tree`, filters out directories, and
    groups files by the penultimate path segment.
- **Impact**
  - The UI no longer reflects the actual workspace hierarchy.
- **Required action**
  - Preserve the real directory tree, or explicitly simplify the browser
    contract and document the loss.

### 8.2. Search results lose snippets

- **Current shipped UI**
  - Search results show the path plus a highlighted snippet.
- **PWA state**
  - Search results show only path and score.
- **Impact**
  - The user loses the context needed to decide which hit to open.
- **Required action**
  - Render content snippets from `SearchHit.content`.

### 8.3. Markdown rendering is weaker

- **Current shipped UI**
  - `.md` files are rendered with Markdown.
- **PWA state**
  - The preview splits content into paragraphs and renders plain text.
- **Impact**
  - Markdown documents lose formatting semantics.
- **Required action**
  - Use the existing Markdown renderer for Markdown memory files.

## 9. Jobs surface gaps

### 9.1. Detail fidelity is much lower

- **Current shipped UI**
  - Job detail has Overview, Activity, and Files subtabs.
  - Overview shows transitions, browse link, duration, mode, and metadata.
  - Activity can display persisted job events plus live SSE job events.
  - Files use a recursive tree, not a flat list.
- **PWA state**
  - The detail view is a single mixed panel.
  - It drops the transitions timeline.
  - It drops `browse_url`.
  - It renders job events as a simple list of `level` and `message`.
  - It flattens files into a one-level list.
- **Impact**
  - The user loses structure and several useful navigation paths.
- **Required action**
  - Restore tabbed detail parity and preserve the richer metadata.

### 9.2. Live activity semantics do not match the current UI

- **Current shipped UI**
  - Activity merges persisted job events with the live `job_*` SSE stream.
  - It understands `message`, `tool_use`, `tool_result`, `status`, and
    `result`.
  - It supports a "done" signal for Claude Code jobs.
- **PWA state**
  - `fetchJobEvents()` assumes a simplified `events: JobEventInfo[]` payload.
  - No live `job_*` stream integration exists.
  - No done signal exists.
- **Impact**
  - The current operational model for long-running jobs is not preserved.
- **Required action**
  - Decide whether job activity should remain stream-driven.
  - If yes, the PWA needs both the live events and the richer persisted event
    shape.

### 9.3. Job metadata is partially ignored

- **Current gateway payload**
  - `JobDetailResponse` includes `project_dir`, `browse_url`, `job_mode`,
    `transitions`, `can_restart`, `can_prompt`, and `job_kind`.
- **PWA state**
  - Only a subset is shown.
  - `browse_url` and `transitions` are ignored.
  - `job_kind` and `job_mode` are collapsed into a generic source pill.
- **Impact**
  - Important troubleshooting and navigation cues are missing.
- **Required action**
  - Surface these fields explicitly in the detail view.

## 10. Routines surface gaps

### 10.1. The detail view drops structured routine configuration

- **Current shipped UI**
  - Detail includes trigger JSON, action JSON, and recent runs with token use
    and optional job links.
- **PWA state**
  - Detail shows only description, a few metadata fields, and a flattened runs
    list.
  - `guardrails`, `notify`, and the structured `trigger` and `action` payloads
    are not shown.
- **Impact**
  - The user loses visibility into what the routine actually does.
- **Required action**
  - Restore the structured configuration sections or provide a more readable
    equivalent.

### 10.2. Run history is simplified too far

- **Current gateway payload**
  - Each run can include `trigger_type`, `status`, `result_summary`,
    `tokens_used`, and `job_id`.
- **PWA state**
  - The runs panel shows only status, start time, and summary text.
- **Impact**
  - The UI loses token accounting and the link back to spawned jobs.
- **Required action**
  - Render `tokens_used` and `job_id` linkage.

## 11. Extensions surface gaps

### 11.1. The install request shape is too narrow

- **Current gateway contract**
  - Extension install accepts `name`, optional `url`, and optional `kind`.
- **PWA state**
  - `installExtension(name)` always posts only `{ name }`.
  - The mock backend also ignores `kind` and `url`.
- **Impact**
  - The PWA cannot express several real install flows, especially typed or
    URL-backed ones.
- **Required action**
  - Use the real install request shape in the client and the mock backend.

### 11.2. MCP add-server flow is under-specified

- **Current shipped UI**
  - The current UI has explicit MCP server install fields and sends `name`,
    `url`, and `kind`.
- **PWA state**
  - The "Add MCP server" panel captures only a name and then calls the narrow
    install helper.
- **Impact**
  - The PWA cannot reproduce the current MCP install flow.
- **Required action**
  - Add the missing endpoint and kind inputs, or remove the add-server panel
    until the flow is real.

### 11.3. Channel-specific activation and pairing details are missing

- **Current shipped UI**
  - WASM channels have a stepper, pairing state, activation error display,
    and pairing-request listing.
- **PWA state**
  - The installed-extension card is generic.
  - There is no pairing surface at all.
- **Impact**
  - The most nuanced extension lifecycle is flattened into generic configure,
    activate, and remove buttons.
- **Required action**
  - Reintroduce the channel-specific lifecycle states if WASM channels remain a
    supported browser surface.

### 11.4. Tools provenance is simplified

- **Current shipped UI**
  - The tools table shows tool name and description within the broader
    extension-management surface.
- **PWA state**
  - The tools table invents a "mock" versus "core" source rule from whether the
    tool name contains an underscore.
- **Impact**
  - The provenance shown to the user is heuristic, not contractual.
- **Required action**
  - Replace the heuristic with a real backend field if source display matters.

## 12. Skills surface gaps

### 12.1. The PWA invents upload and disable flows

- **Current shipped UI**
  - Skills support search, install, and remove.
  - The current UI does not expose a "disable" action for installed skills.
- **PWA state**
  - Installed skills render a "Disable" button that has no real behaviour.
  - The upload panel synthesizes inline mock content instead of using a real
    upload or file-selection flow.
- **Impact**
  - The PWA is showing user actions that do not correspond to the current
    browser contract.
- **Required action**
  - Remove placeholder actions or back them with real gateway work before
    adoption.

### 12.2. Installed-skill detail is too shallow

- **Current shipped UI**
  - Installed skills display trust, version, source, keywords, and remove rules
    with trusted-skill protection.
- **PWA state**
  - The detail view is mostly descriptive and uses keywords as a faux files
    list.
- **Impact**
  - The information model is presentation-led rather than contract-led.
- **Required action**
  - Rework the detail panel around the real installed-skill metadata rather
    than placeholder "files".

### 12.3. Search-result semantics are partly faithful, partly invented

- **Current shipped UI**
  - Search shows registry hits, installed hits, and registry errors.
- **PWA state**
  - This is broadly preserved, but the UI adds format classifications such as
    "bundle", "single", and "preview" based on string heuristics over the
    source or slug.
- **Impact**
  - The PWA is adding classification semantics that do not come from the
    backend.
- **Required action**
  - Either promote these classifications into real backend fields or remove the
    heuristic labels.

## 13. Mock-backend-specific drift

### 13.1. The mock backend is hiding real integration gaps

The current Bun mock backend is not only simulating the gateway. In a few
places it has already drifted into a separate contract:

- it provides `/api/features`, which the real gateway does not
- it uses the wrong logs shape (`source`)
- it imports the wrong job prompt shape (`prompt`)
- it narrows extension install to just `{ name }`
- it does not model auth, token propagation, pairing, TEE, restart, or the
  real route surface

That means "works in mock preview" is not yet evidence that the PWA is aligned
with the real gateway.

## 14. Recommended work order

The front-end developer should address the gaps in this order.

1. Fix the hard transport and payload blockers:
   - auth flow
   - `/api/features` dependency
   - log entry shape
   - job prompt shape
2. Align the PWA route surface with the shipped browser product:
   - logs
   - TEE
   - restart
   - pairing
   - settings and project-browser decisions
3. Restore chat parity:
   - thread metadata
   - pagination
   - auth-required UI
   - job-start and image-generated handling
4. Restore jobs and routines detail fidelity.
5. Tighten extensions and skills so the UI stops advertising placeholder
   actions.
6. Only then treat the Bun mock backend as a confidence harness again.

## 15. Closure criteria

The PWA should not be considered contract-aligned until all of the following
are true:

- every browser API used by the PWA exists in the real gateway
- every browser payload type is derived from or checked against the real Rust
  DTOs
- the PWA no longer drops user-visible information that the current UI presents
  unless that reduction is explicitly accepted as a product change
- the mock backend stops papering over known mismatches
- end-to-end browser tests run against the real gateway for the adopted route
  families, not only against the mock backend
