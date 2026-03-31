# Add missing `builder` field and update E2E extensions tab navigation

## Summary

- Source commit: `b9e5acf66e44fcb7e38c795cbdf96ea0ded553cf`
- Source date: `2026-03-18`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow tests.

## What the upstream commit addressed

Upstream commit `b9e5acf66e44fcb7e38c795cbdf96ea0ded553cf` addresses add missing
`builder` field and update e2e extensions tab navigation. The source subject is
`fix: add missing builder field and update E2E extensions tab navigation
(#1400)`.

Changed upstream paths:

- tests/e2e/scenarios/test_telegram_hot_activation.py
- tests/e2e_telegram_message_routing.rs

Upstream stats:

```text
 tests/e2e/scenarios/test_telegram_hot_activation.py | 5 +++--
 tests/e2e_telegram_message_routing.rs               | 1 +
 2 files changed, 4 insertions(+), 2 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow tests.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow tests) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/tests/e2e/scenarios/test_telegram_hot_activation.py b/tests/e2e/scenarios/test_telegram_hot_activation.py
index e6fa598a..af85b989 100644
--- a/tests/e2e/scenarios/test_telegram_hot_activation.py
+++ b/tests/e2e/scenarios/test_telegram_hot_activation.py
@@ -35,6 +35,7 @@ _TELEGRAM_ACTIVE = {
 
 async def go_to_extensions(page):
-    await page.locator(SEL["tab_button"].format(tab="extensions")).click()
-    await page.locator(SEL["tab_panel"].format(tab="extensions")).wait_for(
+    await page.locator(SEL["tab_button"].format(tab="settings")).click()
+    await page.locator(SEL["settings_subtab"].format(subtab="extensions")).click()
+    await page.locator(SEL["settings_subpanel"].format(subtab="extensions")).wait_for(
         state="visible", timeout=5000
     )
diff --git a/tests/e2e_telegram_message_routing.rs b/tests/e2e_telegram_message_routing.rs
index cad2387c..a96aabe4 100644
--- a/tests/e2e_telegram_message_routing.rs
+++ b/tests/e2e_telegram_message_routing.rs
@@ -199,4 +199,5 @@ mod tests {
             transcription: None,
             document_extraction: None,
+            builder: None,
         };
 
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
