# Eliminate panic paths in production code

## Summary

- Source commit: `716629809cb8d3695e8342c3ade39fb211494837`
- Source date: `2026-03-15`
- Severity: `high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad crates, agent runtime.

## What the upstream commit addressed

Upstream commit `716629809cb8d3695e8342c3ade39fb211494837` (`fix: eliminate
panic paths in production code (#1184)`) addresses eliminate panic paths in
production code.

Changed upstream paths:

- crates/ironclaw_safety/src/leak_detector.rs
- crates/ironclaw_safety/src/policy.rs
- crates/ironclaw_safety/src/sanitizer.rs
- src/agent/session.rs
- src/channels/signal.rs
- src/document_extraction/extractors.rs
- src/extensions/manager.rs
- src/llm/reasoning.rs
- src/llm/registry.rs
- src/llm/smart_routing.rs
- src/settings.rs
- src/setup/channels.rs
- src/skills/mod.rs
- src/tools/builtin/job.rs
- src/tools/mcp/http_transport.rs
- src/tools/wasm/wrapper.rs
- src/workspace/chunker.rs

Upstream stats:

```text
 crates/ironclaw_safety/src/leak_detector.rs |  32 +++++++++---------
 crates/ironclaw_safety/src/policy.rs        | 156 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++-------------------------------
 crates/ironclaw_safety/src/sanitizer.rs     |  12 +++----
 src/agent/session.rs                        |   7 ++--
 src/channels/signal.rs                      |   2 +-
 src/document_extraction/extractors.rs       |   3 +-
 src/extensions/manager.rs                   |  22 ++++++++-----
 src/llm/reasoning.rs                        |   8 ++---
 src/llm/registry.rs                         |   2 +-
 src/llm/smart_routing.rs                    |  43 ++++++++++++------------
 src/settings.rs                             |   9 ++----
 src/setup/channels.rs                       |   2 +-
 src/skills/mod.rs                           |   8 ++---
 src/tools/builtin/job.rs                    |  31 +++++++++++++++++-
 src/tools/mcp/http_transport.rs             |   2 +-
 src/tools/wasm/wrapper.rs                   |   2 +-
 src/workspace/chunker.rs                    |   5 +--
 17 files changed, 216 insertions(+), 130 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad crates,
agent runtime.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `high`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad crates, agent runtime) means the fix could
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
