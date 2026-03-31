# Channel-relay auth dead-end, observability, and URL override

## Summary

- Source commit: `adf4e25c8fdebeacb4bb99752861b61f556dc8db`
- Source date: `2026-03-26`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: broad relay, web gateway.

## What the upstream commit addressed

Upstream commit `adf4e25c8fdebeacb4bb99752861b61f556dc8db` (`fix(extensions):
channel-relay auth dead-end, observability, and URL override (#1681)`) addresses
channel-relay auth dead-end, observability, and url override.

Changed upstream paths:

- src/channels/relay/client.rs
- src/channels/web/server.rs
- src/extensions/manager.rs

Upstream stats:

```text
 src/channels/relay/client.rs |  67 +++++++++++++++--
 src/channels/web/server.rs   |  53 ++++++++++++-
 src/extensions/manager.rs    | 419 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--------
 3 files changed, 500 insertions(+), 39 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad relay, web
gateway.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad relay, web gateway) means the fix could touch
  more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
