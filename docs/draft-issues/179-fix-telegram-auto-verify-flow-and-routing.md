# Fix Telegram auto-verify flow and routing

## Summary

- Source commit: `4675e9618c2f35e803c76476197fbb1d85059f43`
- Source date: `2026-03-16`
- Severity: `medium-high`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: very broad parity docs, agent runtime.

## What the upstream commit addressed

Upstream commit `4675e9618c2f35e803c76476197fbb1d85059f43` (`Fix Telegram
auto-verify flow and routing (#1273)`) addresses fix telegram auto-verify flow
and routing.

Changed upstream paths:

- FEATURE_PARITY.md
- src/agent/agent_loop.rs
- src/channels/web/server.rs
- src/channels/web/static/app.js
- src/channels/web/static/i18n/en.js
- src/channels/web/static/i18n/zh-CN.js
- src/channels/web/static/style.css
- src/extensions/manager.rs
- src/main.rs
- src/tools/builtin/message.rs
- src/tools/registry.rs
- tests/e2e/scenarios/test_extensions.py
- tests/e2e/scenarios/test_telegram_hot_activation.py
- tests/e2e_advanced_traces.rs
- tests/e2e_telegram_message_routing.rs
- tests/fixtures/llm_traces/advanced/routine_event_any_channel.json
- tests/fixtures/llm_traces/advanced/routine_event_telegram.json
- tests/support/test_rig.rs

Upstream stats:

```text
 FEATURE_PARITY.md                                                 |   2 +-
 src/agent/agent_loop.rs                                           | 138 +++++++++++++++++++++-----
 src/channels/web/server.rs                                        |  95 ++++++++++++++++--
 src/channels/web/static/app.js                                    | 153 +++++++++++++++++++++++------
 src/channels/web/static/i18n/en.js                                |   6 +-
 src/channels/web/static/i18n/zh-CN.js                             |   6 ++
 src/channels/web/static/style.css                                 |  22 +++++
 src/extensions/manager.rs                                         | 176 ++++++++++++++++++++++++++++++---
 src/main.rs                                                       |   2 +-
 src/tools/builtin/message.rs                                      | 113 ++++++++++++++++-----
 src/tools/registry.rs                                             |   7 +-
 tests/e2e/scenarios/test_extensions.py                            |  30 ++++++
 tests/e2e/scenarios/test_telegram_hot_activation.py               |  21 ++--
 tests/e2e_advanced_traces.rs                                      |  34 ++++++-
 tests/e2e_telegram_message_routing.rs                             | 353 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
 tests/fixtures/llm_traces/advanced/routine_event_any_channel.json |   8 --
 tests/fixtures/llm_traces/advanced/routine_event_telegram.json    |   8 --
 tests/support/test_rig.rs                                         |   2 +-
 18 files changed, 1045 insertions(+), 131 deletions(-)
 create mode 100644 tests/e2e_telegram_message_routing.rs
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium-high` severity. That keeps
the underlying failure mode close enough to Axinite's current runtime to justify
a follow-up review. The recorded blast radius upstream was very broad parity
docs, agent runtime.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad parity docs, agent runtime) means the fix
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
