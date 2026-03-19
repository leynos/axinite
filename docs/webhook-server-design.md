# Webhook server design

## Front matter

- **Status:** Living design note for the current unified webhook server.
- **Scope:** The host-side `WebhookServer`, its route composition model, its
  restart and shutdown behaviour, and the way the main runtime uses it today.
- **Primary audience:** Maintainers changing HTTP ingress, webhook delivery,
  WebAssembly (WASM) channel hosting, or hot-reload behaviour.
- **Precedence:** `src/NETWORK_SECURITY.md` remains the source of truth for
  network-facing security posture. This document explains the current runtime
  shape and failure behaviour of the webhook server itself.

## 1. Design goal

Axinite currently runs one host process that can expose several HTTP-facing
surfaces at once. Rather than letting each channel or extension start its own
listener, the runtime uses a single `WebhookServer` to host all webhook route
fragments behind one Axum server.

That design is doing three practical jobs:

1. it keeps the process model simple for single-host deployments;
2. it lets built-in and runtime-loaded channels share one ingress surface; and
3. it gives the host one place to implement listener restart and shutdown
   behaviour.

The webhook server is intentionally narrower than the broader hot-reload path
in `src/main.rs`. The server owns *how* a listener starts, stops, or rebinds.
The caller in `main.rs` still owns *when* a restart is attempted and *why*
configuration changes should trigger one.

## 2. Current architecture

The current webhook architecture is deliberately centralized.

- Channels and channel-like subsystems build Axum `Router` fragments.
- Each fragment is expected to have any required state attached before it is
  handed to the webhook server.
- `WebhookServer` accumulates those fragments with `add_routes()`.
- `start()` merges the fragments into one Axum application, binds the listener,
  and spawns the serving task.
- `restart_with_addr()` reuses the already-merged router when the bind address
  changes.

This means webhook ingress is shared across:

- the built-in HTTP channel; and
- WASM channels that contribute webhook routes at runtime.

The main runtime wires this in `src/main.rs` by collecting route fragments,
building a single `WebhookServer`, and then storing it behind
`Arc<tokio::sync::Mutex<_>>` so the SIGHUP reload path can restart it later.

## 3. Route composition model

`src/channels/webhook_server.rs` keeps the composition model intentionally
small.

Table 1. Core server responsibilities.

<!-- markdownlint-disable MD013 MD060 -->
| Component | Responsibility |
| --------- | -------------- |
| `WebhookServerConfig` | Holds the bind address |
| `WebhookServer.routes` | Temporary queue of route fragments before first start |
| `WebhookServer.merged_router` | Saved merged Axum router reused for restarts |
| `WebhookServer.shutdown_tx` | One-shot graceful shutdown trigger for the live listener |
| `WebhookServer.handle` | Join handle for the spawned server task |
<!-- markdownlint-enable MD013 MD060 -->

The important current constraints are:

- route fragments are accumulated before `start()`;
- `start()` drains the pending route list and stores one merged router for
  later reuse;
- restart does not rebuild routes from channel state, it reuses the stored
  merged router; and
- the server is a host runtime utility, not a general router factory.

That last point matters. The route fragments come from other subsystems, but
the webhook server does not try to understand their semantics. It is a
listener-and-lifecycle component.

## 4. Lifecycle and state transitions

The runtime behaviour today is:

1. construct `WebhookServer` with a `SocketAddr`;
2. call `add_routes()` for each prepared fragment;
3. call `start()` once to merge routes, bind, and spawn the server task;
4. optionally call `restart_with_addr()` later if the configured address
   changes; and
5. call `shutdown()` during process teardown.

This gives the server a small internal state machine even though it is not
formalized as an enum:

- pre-start: pending route fragments, no live listener;
- running: merged router saved, shutdown channel installed, task handle live;
- restarting: old listener state retained until the new bind succeeds;
- shut down: no live shutdown sender or task handle.

The implementation is intentionally conservative about shutdown:

- `bind_and_spawn()` creates a one-shot shutdown channel for each live
  listener;
- the serving task uses Axum's graceful shutdown path; and
- `shutdown()` signals the live listener and awaits its task handle.

## 5. Explicit rollback-focused restart behaviour

The most important current design feature is the restart path in
`restart_with_addr()`. It is explicitly rollback-oriented.

The method does **not** shut down the old listener first and hope the new bind
works. Instead, it:

1. clones the previously merged router;
2. snapshots the current address, shutdown sender, and task handle;
3. updates the configured address in memory;
4. attempts to bind and spawn the new listener; and only then
5. shuts down and awaits the old listener if the new one came up
   successfully.

If the new bind fails, the method restores the old address, old shutdown
sender, and old task handle, then returns the bind error. In other words, the
old listener remains active.

That rollback bias is important operationally because it avoids turning an
invalid reload target into an immediate denial of service. A bad port, an
already-in-use address, or another bind failure does not automatically take the
working listener down.

This behaviour is directly exercised by the current tests in
`src/channels/webhook_server.rs`:

- `test_restart_with_addr_rebinds_listener()` verifies that a successful
  restart moves traffic to the new address and that the old address stops
  responding; and
- `test_restart_with_addr_rollback_on_bind_failure()` verifies that a failed
  restart leaves the old listener serving traffic and restores the previous
  address in server state.

## 6. Relationship to hot reload

The webhook server and the SIGHUP handler in `src/main.rs` have different
responsibilities and should be understood separately.

`WebhookServer` owns:

- listener bind and spawn;
- graceful shutdown wiring;
- address rebinding;
- rollback on bind failure; and
- reporting the current address.

The SIGHUP path in `src/main.rs` owns:

- loading updated configuration;
- injecting secrets into the config overlay;
- deciding whether the bind address changed;
- deciding whether a restart is needed; and
- updating channel secrets after a successful reload.

This split is useful, but it is not perfect. The caller currently holds the
server mutex across `restart_with_addr().await`, which means the rollback-safe
server behaviour exists inside a broader hot-reload path that still mixes
configuration policy, secret handling, and transport mutation in one place.

That distinction matters when debugging incidents:

- if the question is “does the listener rollback on bind failure?”, the answer
  lives in `WebhookServer`; but
- if the question is “why did the runtime try to restart at all?”, the answer
  lives in `main.rs`.

## 7. Current trade-offs

The present design is pragmatic, but it comes with trade-offs.

- A single shared listener keeps deployment simple, but it also means ingress
  routing for built-in channels and dynamically loaded WASM channels is
  co-hosted behind one server object.
- Reusing a saved merged router makes restart cheap and deterministic, but it
  also means restart is not the same thing as route recomposition.
- The server itself has a clean rollback story, but the broader reload caller
  still has lock-scope and orchestration complexity around it.
- Storing route fragments until `start()` keeps the API simple, but it assumes
  callers finish route setup before the first bind.

None of those trade-offs are inherently wrong for the current system. They are
simply the shape maintainers need to preserve or revise deliberately.

## 8. Maintainer guidance

When changing webhook behaviour, treat these as the current invariants:

- channels contribute route fragments, but do not own listeners;
- the unified listener must stay restartable without recomputing all route
  state;
- failed address changes should prefer rollback over downtime; and
- listener mechanics and reload policy should stay conceptually separate even
  if the current caller still mixes them operationally.

Before refactoring the restart path, add or preserve characterization coverage
for:

- successful rebind to a new address;
- failed rebind with old-listener rollback; and
- clean shutdown after the server has been restarted.

## 9. References

- `src/channels/webhook_server.rs`
- `src/main.rs`
- `src/channels/mod.rs`
- `docs/axinite-architecture-overview.md`
- `src/NETWORK_SECURITY.md`
