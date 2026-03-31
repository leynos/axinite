# Full_job routine runs stay running until linked job completion

## Summary

- Source commit: `14abd609179a66cc735f2342fa92cdc60bfc0bd9`
- Source date: `2026-03-18`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: very broad agent runtime, database.

## What the upstream commit addressed

Upstream commit `14abd609179a66cc735f2342fa92cdc60bfc0bd9` (`fix: full_job
routine runs stay running until linked job completion (#1374)`) addresses
full_job routine runs stay running until linked job completion.

Changed upstream paths:

- src/agent/routine_engine.rs
- src/db/libsql/routines.rs
- src/db/mod.rs
- src/db/postgres.rs
- src/history/store.rs
- tests/dispatched_routine_run_tests.rs

Upstream stats:

```text
 src/agent/routine_engine.rs           | 348 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++---
 src/db/libsql/routines.rs             |  24 +++++++
 src/db/mod.rs                         |   3 +
 src/db/postgres.rs                    |   4 ++
 src/history/store.rs                  |  12 ++++
 tests/dispatched_routine_run_tests.rs | 360 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 6 files changed, 740 insertions(+), 11 deletions(-)
 create mode 100644 tests/dispatched_routine_run_tests.rs
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad agent
runtime, database.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad agent runtime, database) means the fix could
  touch more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
