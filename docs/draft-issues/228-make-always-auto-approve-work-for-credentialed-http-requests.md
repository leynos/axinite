# Make "always" auto-approve work for credentialed HTTP requests

## Summary

- Source commit: `09e1c97a27bf58760e161fbefb76f3d2085faffc`
- Source date: `2026-03-19`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: very broad agent runtime, src.

## What the upstream commit addressed

Upstream commit `09e1c97a27bf58760e161fbefb76f3d2085faffc` (`fix(approval): make
"always" auto-approve work for credentialed HTTP requests (#1257)`) addresses
make "always" auto-approve work for credentialed http requests.

Changed upstream paths:

- src/agent/dispatcher.rs
- src/agent/session.rs
- src/agent/submission.rs
- src/agent/thread_ops.rs
- src/channels/channel.rs
- src/channels/relay/channel.rs
- src/channels/repl.rs
- src/channels/signal.rs
- src/channels/wasm/wrapper.rs
- src/channels/web/mod.rs
- src/channels/web/static/app.js
- src/channels/web/types.rs
- src/tools/builtin/http.rs

Upstream stats:

```text
 src/agent/dispatcher.rs        | 46 +++++++++++++++++++++++++++++++++++++++-------
 src/agent/session.rs           | 11 +++++++++++
 src/agent/submission.rs        |  2 ++
 src/agent/thread_ops.rs        | 28 ++++++++++++++++++++--------
 src/channels/channel.rs        |  5 +++++
 src/channels/relay/channel.rs  |  4 ++++
 src/channels/repl.rs           | 11 ++++++++---
 src/channels/signal.rs         | 14 +++++++++++---
 src/channels/wasm/wrapper.rs   | 35 ++++++++++++++++++++++++++---------
 src/channels/web/mod.rs        |  2 ++
 src/channels/web/static/app.js | 13 +++++++------
 src/channels/web/types.rs      |  3 +++
 src/tools/builtin/http.rs      | 76 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++---------------
 13 files changed, 199 insertions(+), 51 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad agent
runtime, src.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad agent runtime, src) means the fix could
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
