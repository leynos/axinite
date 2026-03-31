# Fix subagent monitor events being treated as user input

## Summary

- Source commit: `c4e098d4e3d693b3285425ec30baec72588fc80d`
- Source date: `2026-03-15`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: broad Cargo.lock, agent runtime.

## What the upstream commit addressed

Upstream commit `c4e098d4e3d693b3285425ec30baec72588fc80d` (`Fix subagent
monitor events being treated as user input (#1173)`) addresses fix subagent
monitor events being treated as user input.

Changed upstream paths:

- Cargo.lock
- src/agent/agent_loop.rs
- src/agent/dispatcher.rs
- src/agent/job_monitor.rs
- src/tools/builtin/job.rs

Upstream stats:

```text
 Cargo.lock               |  4 ++--
 src/agent/agent_loop.rs  | 17 +++++++++++++++++
 src/agent/dispatcher.rs  |  6 ++++++
 src/agent/job_monitor.rs | 82 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--------------
 src/tools/builtin/job.rs | 45 ++++++++++++++++++++++++++++++++++++++++++++-
 5 files changed, 137 insertions(+), 17 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was broad Cargo.lock, agent
runtime.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (broad Cargo.lock, agent runtime) means the fix could
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
