# N+1 query pattern in event trigger loop (routine_engine)

## Summary

- Source commit: `994a0b194fd3b59db9daa3e3b75ade71940205bd`
- Source date: `2026-03-14`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: very broad agent runtime, database.

## What the upstream commit addressed

Upstream commit `994a0b194fd3b59db9daa3e3b75ade71940205bd` (`fix: N+1 query
pattern in event trigger loop (routine_engine) (#1163)`) addresses n+1 query
pattern in event trigger loop (routine_engine).

Changed upstream paths:

- src/agent/routine_engine.rs
- src/db/libsql/routines.rs
- src/db/mod.rs
- src/db/postgres.rs
- src/history/store.rs
- tests/batch_query_tests.rs
- tests/e2e/scenarios/test_routine_event_batch.py

Upstream stats:

```text
 src/agent/routine_engine.rs                     |  64 +++++++++-
 src/db/libsql/routines.rs                       |  57 ++++++++-
 src/db/mod.rs                                   |   4 +
 src/db/postgres.rs                              |   9 ++
 src/history/store.rs                            |  39 +++++++
 tests/batch_query_tests.rs                      | 509 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 tests/e2e/scenarios/test_routine_event_batch.py | 534 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 7 files changed, 1211 insertions(+), 5 deletions(-)
 create mode 100644 tests/batch_query_tests.rs
 create mode 100644 tests/e2e/scenarios/test_routine_event_batch.py
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
