# Drain tunnel pipes to prevent zombie process

## Summary

- Source commit: `5879d06447726a5bfe61566d81d347e2ca3d3be3`
- Source date: `2026-03-12`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate src.

## What the upstream commit addressed

Upstream commit `5879d06447726a5bfe61566d81d347e2ca3d3be3` (`fix: drain tunnel
pipes to prevent zombie process (#735)`) addresses drain tunnel pipes to prevent
zombie process.

Changed upstream paths:

- src/tunnel/cloudflare.rs
- src/tunnel/custom.rs
- src/tunnel/ngrok.rs

Upstream stats:

```text
 src/tunnel/cloudflare.rs | 38 +++++++++++++++++++++++++++++++++++++-
 src/tunnel/custom.rs     | 42 +++++++++++++++++++++++++++++++++++++++++-
 src/tunnel/ngrok.rs      | 39 +++++++++++++++++++++++++++++++++++++--
 3 files changed, 115 insertions(+), 4 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate src.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate src) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
