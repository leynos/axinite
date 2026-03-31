# Fix conflict

## Summary

- Source commit: `df8bb077378795254e698e088c4009815b9fa489`
- Source date: `2026-03-16`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate crates.

## What the upstream commit addressed

Upstream commit `df8bb077378795254e698e088c4009815b9fa489` (`fix conflict
(#1190)`) addresses fix conflict.

Changed upstream paths:

- crates/ironclaw_safety/src/credential_detect.rs
- crates/ironclaw_safety/src/leak_detector.rs
- crates/ironclaw_safety/src/lib.rs
- crates/ironclaw_safety/src/policy.rs
- crates/ironclaw_safety/src/sanitizer.rs
- crates/ironclaw_safety/src/validator.rs

Upstream stats:

```text
 crates/ironclaw_safety/src/credential_detect.rs | 256 +++++++++++++++++++++++++++++++++++++++++++
 crates/ironclaw_safety/src/leak_detector.rs     | 499 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 crates/ironclaw_safety/src/lib.rs               |  96 ++++++++++++++++
 crates/ironclaw_safety/src/policy.rs            | 232 +++++++++++++++++++++++++++++++++++++++
 crates/ironclaw_safety/src/sanitizer.rs         | 291 +++++++++++++++++++++++++++++++++++++++++++++++++
 crates/ironclaw_safety/src/validator.rs         | 305 +++++++++++++++++++++++++++++++++++++++++++++++++++
 6 files changed, 1679 insertions(+)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate crates.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate crates) means the fix could touch more
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
