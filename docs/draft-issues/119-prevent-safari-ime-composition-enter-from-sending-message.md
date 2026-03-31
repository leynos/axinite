# Prevent Safari IME composition Enter from sending message

## Summary

- Source commit: `a70e58f44e653ea0452e8f5a5c73c3c20f13c2a8`
- Source date: `2026-03-15`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow web gateway.

## What the upstream commit addressed

Upstream commit `a70e58f44e653ea0452e8f5a5c73c3c20f13c2a8` (`fix(web): prevent
Safari IME composition Enter from sending message (#1140)`) addresses prevent
safari ime composition enter from sending message.

Changed upstream paths:

- src/channels/web/static/app.js

Upstream stats:

```text
 src/channels/web/static/app.js | 5 ++++-
 1 file changed, 4 insertions(+), 1 deletion(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow web gateway.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow web gateway) means the fix could touch more
  behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/channels/web/static/app.js b/src/channels/web/static/app.js
index 081b0f3a..ceab682a 100644
--- a/src/channels/web/static/app.js
+++ b/src/channels/web/static/app.js
@@ -1760,5 +1760,8 @@ chatInput.addEventListener('keydown', (e) => {
   }
 
-  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
+  // Safari fires compositionend before keydown, so e.isComposing is already false
+  // when Enter confirms IME input. keyCode 229 (VK_PROCESS) catches this case.
+  // See https://bugs.webkit.org/show_bug.cgi?id=165004
+  if (e.key === 'Enter' && !e.shiftKey && !e.isComposing && e.keyCode !== 229) {
     e.preventDefault();
     hideSlashAutocomplete();
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
