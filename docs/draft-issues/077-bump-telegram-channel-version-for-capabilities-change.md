# Bump telegram channel version for capabilities change

## Summary

- Source commit: `a89cf379938b1fdc58a6ecb11233f5ae90e786eb`
- Source date: `2026-03-12`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow registry.

## What the upstream commit addressed

Upstream commit `a89cf379938b1fdc58a6ecb11233f5ae90e786eb` (`fix(registry): bump
telegram channel version for capabilities change (#1064)`) addresses bump
telegram channel version for capabilities change.

Changed upstream paths:

- registry/channels/telegram.json

Upstream stats:

```text
 registry/channels/telegram.json | 4 ++--
 1 file changed, 2 insertions(+), 2 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow registry.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow registry) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/registry/channels/telegram.json b/registry/channels/telegram.json
index 74336e41..9a4d8918 100644
--- a/registry/channels/telegram.json
+++ b/registry/channels/telegram.json
@@ -3,5 +3,5 @@
   "display_name": "Telegram Channel",
   "kind": "channel",
-  "version": "0.2.2",
+  "version": "0.2.3",
   "wit_version": "0.3.0",
   "description": "Talk to your agent through a Telegram bot",
@@ -19,5 +19,5 @@
   "artifacts": {
     "wasm32-wasip2": {
-      "url": "https://github.com/nearai/ironclaw/releases/latest/download/telegram-0.2.2-wasm32-wasip2.tar.gz",
+      "url": "https://github.com/nearai/ironclaw/releases/latest/download/telegram-0.2.3-wasm32-wasip2.tar.gz",
       "sha256": "b9a83d5a2d1285ce0ec116b354336a1f245f893291ccb01dffbcaccf89d72aed"
     }
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
