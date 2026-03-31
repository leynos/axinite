# HTTP webhook secret transmitted in request body rather than via header, docs inconsistency and security concern

## Summary

- Source commit: `8fb2f70258e3dfcd8d16cc29c57e7d80c0734adf`
- Source date: `2026-03-14`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: narrow webhook auth/docs surface. Carry this if
  Axinite still transmits webhook secrets in request bodies anywhere.

## What the upstream commit addressed

Upstream commit `8fb2f70258e3dfcd8d16cc29c57e7d80c0734adf` (`fix: HTTP webhook
secret transmitted in request body rather than via header, docs inconsistency
and security concern (#1162)`) addresses http webhook secret transmitted in
request body rather than via header, docs inconsistency and security concern.

Changed upstream paths:

- src/channels/http.rs
- tests/e2e/conftest.py
- tests/e2e/scenarios/test_webhook.py

Upstream stats:

```text
 src/channels/http.rs                |  26 ++++----
 tests/e2e/conftest.py               |  91 ++++++++++++++++++++++++++
 tests/e2e/scenarios/test_webhook.py | 340 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 3 files changed, 444 insertions(+), 13 deletions(-)
 create mode 100644 tests/e2e/scenarios/test_webhook.py
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow webhook
auth/docs surface. Carry this if Axinite still transmits webhook secrets in
request bodies anywhere.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow webhook auth/docs surface. Carry this if Axinite
  still transmits webhook secrets in request bodies anywhere) means the fix
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
