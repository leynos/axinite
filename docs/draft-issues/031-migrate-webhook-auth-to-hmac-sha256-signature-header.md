# Migrate webhook auth to HMAC-SHA256 signature header

## Summary

- Source commit: `195ff44b1a653750ce3c10dd2214200df097ec0b`
- Source date: `2026-03-11`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow webhook auth path. Header-based HMAC signing is
  materially safer than body-secret transport and aligns with Axinite's
  fail-closed ingress stance.

## What the upstream commit addressed

Upstream commit `195ff44b1a653750ce3c10dd2214200df097ec0b` (`fix(security):
migrate webhook auth to HMAC-SHA256 signature header (#970)`) addresses migrate
webhook auth to hmac-sha256 signature header.

Changed upstream paths:

- .env.example
- src/channels/http.rs

Upstream stats:

```text
 .env.example         |  13 ++++
 src/channels/http.rs | 456 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++------------
 2 files changed, 420 insertions(+), 49 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow webhook auth
path. Header-based HMAC signing is materially safer than body-secret transport
and aligns with Axinite's fail-closed ingress stance.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow webhook auth path. Header-based HMAC signing is
  materially safer than body-secret transport and aligns with Axinite's
  fail-closed ingress stance) means the fix could touch more behaviour than the
  narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
