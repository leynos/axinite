# Block thread_id-based context pollution across users

## Summary

- Source commit: `2094d6e30d6fe7330b423d0bbc00fbe759722c76`
- Source date: `2026-03-12`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow session isolation path. Cross-user context
  pollution is upstream multi-user framing, but the underlying thread/session
  boundary bug is worth checking anywhere shared session state survives.

## What the upstream commit addressed

Upstream commit `2094d6e30d6fe7330b423d0bbc00fbe759722c76` (`fix(agent): block
thread_id-based context pollution across users (#760)`) addresses block
thread_id-based context pollution across users.

Changed upstream paths:

- src/agent/agent_loop.rs
- src/agent/thread_ops.rs
- src/channels/web/handlers/chat.rs
- src/channels/web/server.rs
- src/db/libsql/conversations.rs
- src/db/mod.rs
- src/db/postgres.rs
- src/history/store.rs
- src/testing/mod.rs
- tests/e2e_thread_id_isolation.rs

Upstream stats:

```text
 src/agent/agent_loop.rs           |   4 ++-
 src/agent/thread_ops.rs           | 190 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--------------
 src/channels/web/handlers/chat.rs |  10 ++++--
 src/channels/web/server.rs        |  10 ++++--
 src/db/libsql/conversations.rs    |  15 ++++----
 src/db/mod.rs                     |   2 +-
 src/db/postgres.rs                |   2 +-
 src/history/store.rs              |  24 ++++++++-----
 src/testing/mod.rs                |  62 ++++++++++++++++++++++++++++----
 tests/e2e_thread_id_isolation.rs  | 183 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 10 files changed, 448 insertions(+), 54 deletions(-)
 create mode 100644 tests/e2e_thread_id_isolation.rs
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow session
isolation path. Cross-user context pollution is upstream multi-user framing, but
the underlying thread/session boundary bug is worth checking anywhere shared
session state survives.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow session isolation path. Cross-user context
  pollution is upstream multi-user framing, but the underlying thread/session
  boundary bug is worth checking anywhere shared session state survives) means
  the fix could touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
