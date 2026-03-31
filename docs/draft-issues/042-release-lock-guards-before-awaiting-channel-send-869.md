# Release lock guards before awaiting channel send (#869)

## Summary

- Source commit: `ef34943c14993d4db155d7f6ea07650266732e05`
- Source date: `2026-03-12`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: very broad CI/release, webhooks.

## What the upstream commit addressed

Upstream commit `ef34943c14993d4db155d7f6ea07650266732e05` (`fix: release lock
guards before awaiting channel send (#869) (#1003)`) addresses release lock
guards before awaiting channel send (#869).

Changed upstream paths:

- .github/workflows/regression-test-check.yml
- src/channels/http.rs
- src/channels/wasm/wrapper.rs
- src/channels/web/handlers/chat.rs
- src/channels/web/server.rs
- src/channels/web/ws.rs

Upstream stats:

```text
 .github/workflows/regression-test-check.yml | 17 ++++++++++++-----
 src/channels/http.rs                        | 61 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/channels/wasm/wrapper.rs                | 90 +++++++++++++++++++++++++++++++++++++++++++++++++----------------------------------------
 src/channels/web/handlers/chat.rs           | 32 ++++++++++++++++++++++----------
 src/channels/web/server.rs                  | 32 ++++++++++++++++++++++----------
 src/channels/web/ws.rs                      | 16 ++++++++++++----
 6 files changed, 179 insertions(+), 69 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad CI/release,
webhooks.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad CI/release, webhooks) means the fix could
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
