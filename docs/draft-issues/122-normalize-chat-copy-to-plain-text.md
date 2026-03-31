# Normalize chat copy to plain text

## Summary

- Source commit: `dac420840d01784fb7ca42e655b9a62763933bb9`
- Source date: `2026-03-15`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: moderate web gateway, tests.

## What the upstream commit addressed

Upstream commit `dac420840d01784fb7ca42e655b9a62763933bb9` (`fix(web-chat):
normalize chat copy to plain text (#1114)`) addresses normalize chat copy to
plain text.

Changed upstream paths:

- src/channels/web/static/app.js
- tests/e2e/scenarios/test_chat.py

Upstream stats:

```text
 src/channels/web/static/app.js   | 16 ++++++++++++++++
 tests/e2e/scenarios/test_chat.py | 41 +++++++++++++++++++++++++++++++++++++++++
 2 files changed, 57 insertions(+)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was moderate web gateway,
tests.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (moderate web gateway, tests) means the fix could touch
  more behaviour than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/channels/web/static/app.js b/src/channels/web/static/app.js
index ceab682a..0624d07a 100644
--- a/src/channels/web/static/app.js
+++ b/src/channels/web/static/app.js
@@ -601,4 +601,20 @@ document.getElementById('chat-input').addEventListener('paste', (e) => {
 });
 
+const chatMessagesEl = document.getElementById('chat-messages');
+chatMessagesEl.addEventListener('copy', (e) => {
+  const selection = window.getSelection();
+  if (!selection || selection.isCollapsed) return;
+  const anchorNode = selection.anchorNode;
+  const focusNode = selection.focusNode;
+  if (!anchorNode || !focusNode) return;
+  if (!chatMessagesEl.contains(anchorNode) || !chatMessagesEl.contains(focusNode)) return;
+  const text = selection.toString();
+  if (!text || !e.clipboardData) return;
+  // Force plain-text clipboard output so dark-theme styling never leaks on paste.
+  e.preventDefault();
+  e.clipboardData.clearData();
+  e.clipboardData.setData('text/plain', text);
+});
+
 function addGeneratedImage(dataUrl, path) {
   const container = document.getElementById('chat-messages');
diff --git a/tests/e2e/scenarios/test_chat.py b/tests/e2e/scenarios/test_chat.py
index 24b3d98d..440eb18e 100644
--- a/tests/e2e/scenarios/test_chat.py
+++ b/tests/e2e/scenarios/test_chat.py
@@ -75,2 +75,43 @@ async def test_empty_message_not_sent(page):
     final_count = await page.locator(f"{SEL['message_user']}, {SEL['message_assistant']}").count()
     assert final_count == initial_count, "Empty message should not create new messages"
+
+
+async def test_copy_from_chat_forces_plain_text(page):
+    """Copying selected chat text should populate plain text clipboard data only."""
+    await page.evaluate("addMessage('assistant', 'Copy me into Sheets')")
+
+    copied = await page.evaluate(
+        """
+        () => {
+          const content = Array.from(document.querySelectorAll('#chat-messages .message.assistant .message-content'))
+            .find((el) => (el.textContent || '').includes('Copy me into Sheets'));
+          if (!content) return {ok: false, reason: 'no content'};
+          const range = document.createRange();
+          range.selectNodeContents(content);
+          const sel = window.getSelection();
+          sel.removeAllRanges();
+          sel.addRange(range);
+
+          const store = {};
+          const evt = new Event('copy', { bubbles: true, cancelable: true });
+          evt.clipboardData = {
+            clearData: () => { Object.keys(store).forEach((k) => delete store[k]); },
+            setData: (t, v) => { store[t] = v; },
+            getData: (t) => store[t] || '',
+          };
+
+          content.dispatchEvent(evt);
+          return {
+            ok: true,
+            defaultPrevented: evt.defaultPrevented,
+            text: store['text/plain'] || '',
+            html: store['text/html'] || '',
+          };
+        }
+        """
+    )
+
+    assert copied["ok"], copied.get("reason", "copy setup failed")
+    assert copied["defaultPrevented"] is True
+    assert "Copy me into Sheets" in copied["text"]
+    assert copied["html"] == ""
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
