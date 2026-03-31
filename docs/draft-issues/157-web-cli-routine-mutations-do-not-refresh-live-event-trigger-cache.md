# Web/CLI routine mutations do not refresh live event trigger cache

## Summary

- Source commit: `971b4c2ef43872d87dfbcbecce2587761c1dd860`
- Source date: `2026-03-16`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate web gateway, tests.

## What the upstream commit addressed

Upstream commit `971b4c2ef43872d87dfbcbecce2587761c1dd860` (`fix: web/CLI
routine mutations do not refresh live event trigger cache (#1255)`) addresses
web/cli routine mutations do not refresh live event trigger cache.

Changed upstream paths:

- src/channels/web/server.rs
- tests/e2e_routine_heartbeat.rs

Upstream stats:

```text
 src/channels/web/server.rs     |  79 +---------------------------------------------------------------------
 tests/e2e_routine_heartbeat.rs | 114 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 2 files changed, 115 insertions(+), 78 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate web gateway,
tests.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate web gateway, tests) means the fix could touch
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
