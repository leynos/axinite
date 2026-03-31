# Use versioned artifact URLs and checksums for all WASM manifests

## Summary

- Source commit: `c937dfa315d84017f8b8c01dc1e534a855f1c2a3`
- Source date: `2026-03-12`
- Severity: `medium`
- Main relevance: `yes`
- Effectiveness: `targeted`
- Scope and blast radius: very broad registry.

## What the upstream commit addressed

Upstream commit `c937dfa315d84017f8b8c01dc1e534a855f1c2a3` (`fix(registry): use
versioned artifact URLs and checksums for all WASM manifests (#1007)`) addresses
use versioned artifact urls and checksums for all wasm manifests.

Changed upstream paths:

- registry/channels/discord.json
- registry/channels/slack.json
- registry/channels/telegram.json
- registry/channels/whatsapp.json
- registry/tools/github.json
- registry/tools/gmail.json
- registry/tools/google-calendar.json
- registry/tools/google-docs.json
- registry/tools/google-drive.json
- registry/tools/google-sheets.json
- registry/tools/google-slides.json
- registry/tools/slack.json
- registry/tools/telegram.json
- registry/tools/web-search.json

Upstream stats:

```text
 registry/channels/discord.json      | 4 ++--
 registry/channels/slack.json        | 4 ++--
 registry/channels/telegram.json     | 4 ++--
 registry/channels/whatsapp.json     | 4 ++--
 registry/tools/github.json          | 4 ++--
 registry/tools/gmail.json           | 4 ++--
 registry/tools/google-calendar.json | 4 ++--
 registry/tools/google-docs.json     | 4 ++--
 registry/tools/google-drive.json    | 4 ++--
 registry/tools/google-sheets.json   | 4 ++--
 registry/tools/google-slides.json   | 4 ++--
 registry/tools/slack.json           | 4 ++--
 registry/tools/telegram.json        | 4 ++--
 registry/tools/web-search.json      | 4 ++--
 14 files changed, 28 insertions(+), 28 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was very broad registry.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (very broad registry) means the fix could touch more
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
