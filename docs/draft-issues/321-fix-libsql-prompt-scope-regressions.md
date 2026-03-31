# Fix libsql prompt scope regressions

## Summary

- Source commit: `86d11430640da22d8f890bb9b2df867dda1e668e`
- Source date: `2026-03-25`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: broad agent runtime, workspace/memory.

## What the upstream commit addressed

Upstream commit `86d11430640da22d8f890bb9b2df867dda1e668e` (`Fix libsql prompt
scope regressions (#1651)`) addresses fix libsql prompt scope regressions.

Changed upstream paths:

- src/agent/dispatcher.rs
- src/workspace/mod.rs
- src/workspace/repository.rs
- tests/e2e_workspace_coverage.rs
- tests/multi_tenant_system_prompt.rs

Upstream stats:

```text
 src/agent/dispatcher.rs             |  7 ++++++-
 src/workspace/mod.rs                | 55 +++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/workspace/repository.rs         |  1 +
 tests/e2e_workspace_coverage.rs     |  4 +++-
 tests/multi_tenant_system_prompt.rs | 14 +++++++-------
 5 files changed, 72 insertions(+), 9 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad agent runtime,
workspace/memory.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad agent runtime, workspace/memory) means the fix
  could touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
