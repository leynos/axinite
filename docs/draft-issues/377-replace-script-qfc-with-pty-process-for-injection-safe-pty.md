# Replace script -qfc with pty-process for injection-safe PTY

## Summary

- Source commit: `de5a1c7b0d0588e1898458870a0796e6dd8a361e`
- Source date: `2026-03-29`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `strong`
- Scope and blast radius: medium worker/PTY boundary. Replacing shell-mediated
  PTY spawning with an injection-safe process wrapper is directly relevant to
  Axinite's constrained codemode ambitions.

## What the upstream commit addressed

Upstream commit `de5a1c7b0d0588e1898458870a0796e6dd8a361e` (`fix(worker):
replace script -qfc with pty-process for injection-safe PTY (#1678)`) addresses
replace script -qfc with pty-process for injection-safe pty.

Changed upstream paths:

- Cargo.lock
- Cargo.toml
- src/worker/claude_bridge.rs

Upstream stats:

```text
 Cargo.lock                  |  21 ++++++++++---
 Cargo.toml                  |   4 +++
 src/worker/claude_bridge.rs | 176 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--------------------
 3 files changed, 162 insertions(+), 39 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was medium worker/PTY
boundary. Replacing shell-mediated PTY spawning with an injection-safe process
wrapper is directly relevant to Axinite's constrained codemode ambitions.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `strong` effectiveness in the staging
  audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (medium worker/PTY boundary. Replacing shell-mediated
  PTY spawning with an injection-safe process wrapper is directly relevant to
  Axinite's constrained codemode ambitions) means the fix could touch more
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
