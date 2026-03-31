# Update yanked uds_windows 1.2.0 -> 1.2.1

## Summary

- Source commit: `71b1a6778b93a77a58b3278296a0bb0d8c2bb561`
- Source date: `2026-03-14`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow repo plumbing.

## What the upstream commit addressed

Upstream commit `71b1a6778b93a77a58b3278296a0bb0d8c2bb561` (`fix(deps): update
yanked uds_windows 1.2.0 -> 1.2.1 (#1183)`) addresses update yanked uds_windows
1.2.0 -> 1.2.1.

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow repo plumbing.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow repo plumbing) means the fix could touch more
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
