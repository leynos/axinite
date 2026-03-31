# 5 critical/high-priority bugs (auth bypass, relay failures, unbounded recursion, context growth)

## Summary

- Source commit: `e805ec61aa6e744679cebb73b86bfc5e26ca5e6f`
- Source date: `2026-03-13`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `comprehensive`
- Scope and blast radius: broad but still targeted at core runtime guardrails.
  The depth limits, runtime auth checks, context truncation, and SSRF hardening
  all hit surfaces Axinite still carries.

## What the upstream commit addressed

Upstream commit `e805ec61aa6e744679cebb73b86bfc5e26ca5e6f` (`fix: 5
critical/high-priority bugs (auth bypass, relay failures, unbounded recursion,
context growth) (#1083)`) addresses 5 critical/high-priority bugs (auth bypass,
relay failures, unbounded recursion, context growth).

Changed upstream paths:

- src/agent/routine_engine.rs
- src/channels/http.rs
- src/channels/relay/channel.rs
- src/channels/web/handlers/routines.rs
- src/tools/builtin/http.rs
- src/tools/builtin/routine.rs
- src/tools/tool.rs
- src/tools/wasm/capabilities_schema.rs
- src/worker/job.rs

Upstream stats:

```text
 src/agent/routine_engine.rs           |  43 ++++++++++++++++++++++++----------
 src/channels/http.rs                  |  50 +++++++++++++++++++++++++++++++++-------
 src/channels/relay/channel.rs         |   4 ++++
 src/channels/web/handlers/routines.rs |  27 +++++++++++++++++++---
 src/tools/builtin/http.rs             |   3 +++
 src/tools/builtin/routine.rs          |  10 +++++---
 src/tools/tool.rs                     |  51 ++++++++++++++++++++++++++++++++++++++---
 src/tools/wasm/capabilities_schema.rs | 118 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++----
 src/worker/job.rs                     |   8 +++----
 9 files changed, 277 insertions(+), 37 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad but still
targeted at core runtime guardrails. The depth limits, runtime auth checks,
context truncation, and SSRF hardening all hit surfaces Axinite still carries.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `comprehensive` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad but still targeted at core runtime guardrails.
  The depth limits, runtime auth checks, context truncation, and SSRF hardening
  all hit surfaces Axinite still carries) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
