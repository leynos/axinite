# Managed tunnels target wrong port and die from SIGPIPE

## Summary

- Source commit: `fb3548956bf6b1cc4fb31cb753b4fa24a7cfec68`
- Source date: `2026-03-24`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate src.

## What the upstream commit addressed

Upstream commit `fb3548956bf6b1cc4fb31cb753b4fa24a7cfec68` (`fix(tunnel):
managed tunnels target wrong port and die from SIGPIPE (#1093)`) addresses
managed tunnels target wrong port and die from sigpipe.

Changed upstream paths:

- src/tunnel/cloudflare.rs
- src/tunnel/custom.rs
- src/tunnel/mod.rs
- src/tunnel/ngrok.rs

Upstream stats:

```text
 src/tunnel/cloudflare.rs |  28 +++++++++++++++-------
 src/tunnel/custom.rs     |  23 ++++++++++++++----
 src/tunnel/mod.rs        | 136 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-------------
 src/tunnel/ngrok.rs      |  31 +++++++++++++++++--------
 4 files changed, 179 insertions(+), 39 deletions(-)
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
