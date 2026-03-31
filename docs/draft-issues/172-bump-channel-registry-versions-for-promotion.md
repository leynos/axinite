# Bump channel registry versions for promotion

## Summary

- Source commit: `1f209db0faa8169e2e83dff5b700e30db1aead9f`
- Source date: `2026-03-16`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow registry.

## What the upstream commit addressed

Upstream commit `1f209db0faa8169e2e83dff5b700e30db1aead9f` (`fix: bump channel
registry versions for promotion (#1264)`) addresses bump channel registry
versions for promotion.

Changed upstream paths:

- registry/channels/feishu.json
- registry/channels/telegram.json

Upstream stats:

```text
 registry/channels/feishu.json   | 2 +-
 registry/channels/telegram.json | 2 +-
 2 files changed, 2 insertions(+), 2 deletions(-)
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
diff --git a/registry/channels/feishu.json b/registry/channels/feishu.json
index cbdf7da2..0446a442 100644
--- a/registry/channels/feishu.json
+++ b/registry/channels/feishu.json
@@ -3,5 +3,5 @@
   "display_name": "Feishu / Lark Channel",
   "kind": "channel",
-  "version": "0.1.0",
+  "version": "0.1.1",
   "wit_version": "0.3.0",
   "description": "Talk to your agent through a Feishu or Lark bot",
diff --git a/registry/channels/telegram.json b/registry/channels/telegram.json
index 36be1fc7..e44061e5 100644
--- a/registry/channels/telegram.json
+++ b/registry/channels/telegram.json
@@ -3,5 +3,5 @@
   "display_name": "Telegram Channel",
   "kind": "channel",
-  "version": "0.2.3",
+  "version": "0.2.4",
   "wit_version": "0.3.0",
   "description": "Talk to your agent through a Telegram bot",
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
