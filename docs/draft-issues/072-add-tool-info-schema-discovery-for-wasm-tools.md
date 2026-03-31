# Add tool_info schema discovery for WASM tools

## Summary

- Source commit: `8a60fa2d37793e27d797b4438feec81e8ed8330a`
- Source date: `2026-03-12`
- Severity: `medium-high`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad src, tool runtime.

## What the upstream commit addressed

Upstream commit `8a60fa2d37793e27d797b4438feec81e8ed8330a` (`fix: add tool_info
schema discovery for WASM tools (#1086)`) addresses add tool_info schema
discovery for wasm tools.

Changed upstream paths:

- src/app.rs
- src/tools/builtin/mod.rs
- src/tools/builtin/tool_info.rs
- src/tools/registry.rs
- src/tools/tool.rs
- src/tools/wasm/error.rs
- src/tools/wasm/limits.rs
- src/tools/wasm/mod.rs
- src/tools/wasm/runtime.rs
- src/tools/wasm/wrapper.rs
- tests/e2e_builtin_tool_coverage.rs
- tests/fixtures/llm_traces/tools/tool_info_discovery.json
- tools-src/web-search/web-search-tool.capabilities.json

Upstream stats:

```text
 src/app.rs                                               |   1 +
 src/tools/builtin/mod.rs                                 |   2 +
 src/tools/builtin/tool_info.rs                           | 183 +++++++++++++++++++++++++++++++++++++++++++++
 src/tools/registry.rs                                    |  12 +++
 src/tools/tool.rs                                        |  11 +++
 src/tools/wasm/error.rs                                  |  90 ++---------------------
 src/tools/wasm/limits.rs                                 |   8 --
 src/tools/wasm/mod.rs                                    |   2 +-
 src/tools/wasm/runtime.rs                                |  67 +++++++----------
 src/tools/wasm/wrapper.rs                                | 302 +++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++--------------
 tests/e2e_builtin_tool_coverage.rs                       |  86 ++++++++++++++++++++++
 tests/fixtures/llm_traces/tools/tool_info_discovery.json |  50 +++++++++++++
 tools-src/web-search/web-search-tool.capabilities.json   |  35 +++++++++
 13 files changed, 659 insertions(+), 190 deletions(-)
 create mode 100644 src/tools/builtin/tool_info.rs
 create mode 100644 tests/fixtures/llm_traces/tools/tool_info_discovery.json
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium-high` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad src, tool
runtime.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium-high`-class issue, gives Axinite an upstream
  repair shape to compare against, and comes with `targeted` effectiveness in
  the staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad src, tool runtime) means the fix could touch
  more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is targeted, but the raw diff is large enough that it should
be reviewed separately when this issue is taken forward.

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
