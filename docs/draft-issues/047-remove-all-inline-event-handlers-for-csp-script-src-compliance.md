# Remove all inline event handlers for CSP script-src compliance

## Summary

- Source commit: `f776d96395c1b78db86a7b4704b5861c78dacab0`
- Source date: `2026-03-12`
- Severity: `high`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: very broad CI/release, CHANGELOG.md.

## What the upstream commit addressed

Upstream commit `f776d96395c1b78db86a7b4704b5861c78dacab0` (`fix: remove all
inline event handlers for CSP script-src compliance (#1063)`) addresses remove
all inline event handlers for csp script-src compliance.

Changed upstream paths:

- .github/workflows/e2e.yml
- CHANGELOG.md
- Cargo.lock
- Cargo.toml
- src/channels/web/static/app.js
- src/channels/web/static/index.html
- src/db/libsql/mod.rs
- src/main.rs
- tests/e2e/scenarios/test_csp.py

Upstream stats:

```text
 .github/workflows/e2e.yml          |   2 +-
 CHANGELOG.md                       |   9 ++++++++
 Cargo.lock                         |   2 +-
 Cargo.toml                         |   2 +-
 src/channels/web/static/app.js     | 121 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-------------
 src/channels/web/static/index.html |  54 +++++++++++++++++++++----------------------
 src/db/libsql/mod.rs               |   4 ++--
 src/main.rs                        |   5 ++--
 tests/e2e/scenarios/test_csp.py    |  99 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 9 files changed, 248 insertions(+), 50 deletions(-)
 create mode 100644 tests/e2e/scenarios/test_csp.py
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad CI/release,
CHANGELOG.md.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad CI/release, CHANGELOG.md) means the fix
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
