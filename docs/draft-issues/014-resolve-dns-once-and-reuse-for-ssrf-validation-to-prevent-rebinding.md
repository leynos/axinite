# Resolve DNS once and reuse for SSRF validation to prevent rebinding

## Summary

- Source commit: `bb0656577091ee9e10611f583fa9572d85dccf83`
- Source date: `2026-03-11`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow HTTP/network boundary. DNS result pinning is
  directly relevant to Axinite's MCP-over-HTTPS and delegated-endpoint hardening
  story.

## What the upstream commit addressed

Upstream commit `bb0656577091ee9e10611f583fa9572d85dccf83` (`fix(security):
resolve DNS once and reuse for SSRF validation to prevent rebinding (#518)`)
addresses resolve dns once and reuse for ssrf validation to prevent rebinding.

Changed upstream paths:

- src/tools/builtin/http.rs

Upstream stats:

```text
 src/tools/builtin/http.rs | 410 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++-----------------------
 1 file changed, 322 insertions(+), 88 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow HTTP/network
boundary. DNS result pinning is directly relevant to Axinite's MCP-over-HTTPS
and delegated-endpoint hardening story.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow HTTP/network boundary. DNS result pinning is
  directly relevant to Axinite's MCP-over-HTTPS and delegated-endpoint hardening
  story) means the fix could touch more behaviour than the narrow symptom
  suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
