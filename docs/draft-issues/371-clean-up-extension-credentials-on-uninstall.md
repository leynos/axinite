# Clean up extension credentials on uninstall

## Summary

- Source commit: `f49f3683555de0f65a53918d9feb37ca4d0eeecb`
- Source date: `2026-03-28`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate extensions/registry, tests.

## What the upstream commit addressed

Upstream commit `f49f3683555de0f65a53918d9feb37ca4d0eeecb` (`Clean up extension
credentials on uninstall (#1718)`) addresses clean up extension credentials on
uninstall.

Changed upstream paths:

- src/extensions/manager.rs
- tests/e2e/CLAUDE.md
- tests/e2e/conftest.py
- tests/e2e/scenarios/test_extension_uninstall_cleanup.py

Upstream stats:

```text
 src/extensions/manager.rs                               | 650 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-
 tests/e2e/CLAUDE.md                                     |   2 +
 tests/e2e/conftest.py                                   | 109 +++++++++++++
 tests/e2e/scenarios/test_extension_uninstall_cleanup.py | 266 +++++++++++++++++++++++++++++++
 4 files changed, 1026 insertions(+), 1 deletion(-)
 create mode 100644 tests/e2e/scenarios/test_extension_uninstall_cleanup.py
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate
extensions/registry, tests.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate extensions/registry, tests) means the fix
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
