# Complete full_job execution reliability overhaul

## Summary

- Source commit: `8a320ae9db4f7fdada609a30528bee6116cbe71c`
- Source date: `2026-03-28`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `comprehensive`
- Scope and blast radius: very broad Cargo.toml, agent runtime.

## What the upstream commit addressed

Upstream commit `8a320ae9db4f7fdada609a30528bee6116cbe71c` (`fix(routines):
complete full_job execution reliability overhaul (#1650)`) addresses complete
full_job execution reliability overhaul.

Changed upstream paths:

- Cargo.toml
- src/agent/dispatcher.rs
- src/agent/mod.rs
- src/agent/routine.rs
- src/agent/routine_engine.rs
- src/channels/repl.rs
- src/channels/web/handlers/jobs.rs
- src/channels/web/handlers/routines.rs
- src/channels/web/mod.rs
- src/channels/web/static/app.js
- src/channels/web/tests/mod.rs
- src/channels/web/tests/no_silent_drop.rs
- src/channels/web/types.rs
- src/db/libsql/conversations.rs
- src/db/mod.rs
- src/db/postgres.rs
- src/history/store.rs
- src/llm/openai_codex_provider.rs
- src/skills/catalog.rs
- src/tools/builtin/message.rs
- src/tools/builtin/routine.rs
- src/tools/builtin/time.rs
- src/tools/mcp/auth.rs
- src/tools/mcp/client.rs
- src/tools/mcp/config.rs
- src/util.rs
- src/worker/job.rs
- tests/e2e/ironclaw_e2e.egg-info/SOURCES.txt
- tests/e2e/mock_llm.py
- tests/e2e/scenarios/test_routine_full_job.py
- tests/e2e_routine_heartbeat.rs
- tests/e2e_telegram_message_routing.rs
- tests/support/gateway_workflow_harness.rs
- tests/support/test_rig.rs

Upstream stats:

```text
 Cargo.toml                                   |   3 +-
 src/agent/dispatcher.rs                      |  20 ++++++++++++
 src/agent/mod.rs                             |   1 +
 src/agent/routine.rs                         |   7 ++++-
 src/agent/routine_engine.rs                  |  14 ++++++++-
 src/channels/repl.rs                         |  37 ++++++++--------------
 src/channels/web/handlers/jobs.rs            |  37 +++++++++++++++-------
 src/channels/web/handlers/routines.rs        |  11 +++++++
 src/channels/web/mod.rs                      |  16 +++++-----
 src/channels/web/static/app.js               |  12 ++++++++
 src/channels/web/tests/mod.rs                |   1 +
 src/channels/web/tests/no_silent_drop.rs     |  93 +++++++++++++++++++++++++++++++++++++++++++++++++++++++
 src/channels/web/types.rs                    |   1 +
 src/db/libsql/conversations.rs               |  35 +++++++++++++++++++++
 src/db/mod.rs                                |   7 +++++
 src/db/postgres.rs                           |  10 ++++++
 src/history/store.rs                         |  21 +++++++++++++
 src/llm/openai_codex_provider.rs             | 136 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--
 src/skills/catalog.rs                        |  12 ++++++--
 src/tools/builtin/message.rs                 |  40 ++++++++++++++++++++++--
 src/tools/builtin/routine.rs                 |  72 ++++++++++++++++++++++++++++++++++++++++---
 src/tools/builtin/time.rs                    |   9 ++++--
 src/tools/mcp/auth.rs                        |  35 ++++++++++++++++-----
 src/tools/mcp/client.rs                      |  30 ++++++++++++++++++
 src/tools/mcp/config.rs                      |  19 ++++++++++++
 src/util.rs                                  |  22 +++++++++++++
 src/worker/job.rs                            | 146 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--------------
 tests/e2e/ironclaw_e2e.egg-info/SOURCES.txt  |   3 ++
 tests/e2e/mock_llm.py                        |  82 +++++++++++++++++++++++++++++++++++++++++++++++++
 tests/e2e/scenarios/test_routine_full_job.py | 133 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 tests/e2e_routine_heartbeat.rs               |  22 ++++++-------
 tests/e2e_telegram_message_routing.rs        |   2 +-
 tests/support/gateway_workflow_harness.rs    |   3 +-
 tests/support/test_rig.rs                    |   4 +--
 34 files changed, 988 insertions(+), 108 deletions(-)
 create mode 100644 src/channels/web/tests/no_silent_drop.rs
 create mode 100644 tests/e2e/scenarios/test_routine_full_job.py
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad Cargo.toml,
agent runtime.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `comprehensive` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad Cargo.toml, agent runtime) means the fix
  could touch more behaviour than the narrow symptom suggests.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
