# Restore libSQL vector search with dynamic dimensions

## Summary

- Source commit: `8526cde1be0aa0e34c53aaf6833a80644c1aef97`
- Source date: `2026-03-19`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: broad config, database.

## What the upstream commit addressed

Upstream commit `8526cde1be0aa0e34c53aaf6833a80644c1aef97` (`fix: restore libSQL
vector search with dynamic dimensions (#1393)`) addresses restore libsql vector
search with dynamic dimensions.

Changed upstream paths:

- src/config/embeddings.rs
- src/config/mod.rs
- src/db/CLAUDE.md
- src/db/libsql/mod.rs
- src/db/libsql/workspace.rs
- src/db/libsql_migrations.rs
- src/workspace/README.md

Upstream stats:

```text
 src/config/embeddings.rs    |   2 +-
 src/config/mod.rs           |   2 +-
 src/db/CLAUDE.md            |   6 +-
 src/db/libsql/mod.rs        |   8 ++
 src/db/libsql/workspace.rs  | 481 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--
 src/db/libsql_migrations.rs |  13 ++-
 src/workspace/README.md     |   2 +-
 7 files changed, 494 insertions(+), 20 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad config, database.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad config, database) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
