# Drain residual events and filter key kind in onboard prompts (#937)

## Summary

- Source commit: `6321bb46883fb5fd51fb8236b3e3f427a66586ce`
- Source date: `2026-03-12`
- Severity: `medium`
- Main relevance: `maybe`
- Effectiveness: `targeted`
- Scope and blast radius: narrow src.

## What the upstream commit addressed

Upstream commit `6321bb46883fb5fd51fb8236b3e3f427a66586ce` (`fix(setup): drain
residual events and filter key kind in onboard prompts (#937) (#949)`) addresses
drain residual events and filter key kind in onboard prompts (#937).

Changed upstream paths:

- src/setup/prompts.rs

Upstream stats:

```text
 src/setup/prompts.rs | 40 +++++++++++++++++++++++++++-------------
 1 file changed, 27 insertions(+), 13 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: maybe` with `medium` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow src.

## Compatibility concerns

The upstream patch should be treated as a reference implementation rather than a
cherry-pick candidate. Axinite needs a seam-by-seam verification pass before
adopting it.

## Risks and benefits of the fix

- Benefits: addresses a `medium`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `targeted` effectiveness in the
  staging audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow src) means the fix could touch more behaviour
  than the narrow symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/setup/prompts.rs b/src/setup/prompts.rs
index a52a8b68..d1a3e08c 100644
--- a/src/setup/prompts.rs
+++ b/src/setup/prompts.rs
@@ -12,5 +12,5 @@ use std::io::{self, Write};
 use crossterm::{
     cursor,
-    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
+    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
     execute,
     style::{Color, Print, ResetColor, SetForegroundColor},
@@ -19,4 +19,16 @@ use crossterm::{
 use secrecy::SecretString;
 
+/// Drain any residual key events already queued in the terminal buffer.
+///
+/// On Windows, transitioning between raw mode and cooked mode (or between
+/// successive raw-mode prompts) can leave stale events (e.g. the Release
+/// half of an Enter keypress) in the queue. Consuming them with a
+/// non-blocking poll prevents the next prompt from mis-firing.
+fn drain_pending_events() {
+    while event::poll(std::time::Duration::ZERO).unwrap_or(false) {
+        let _ = event::read();
+    }
+}
+
 /// Display a numbered menu and get user selection.
 ///
@@ -95,4 +107,5 @@ pub fn select_many(prompt: &str, options: &[(&str, bool)]) -> io::Result<Vec<usi
 
     terminal::enable_raw_mode()?;
+    drain_pending_events();
     execute!(stdout, cursor::Hide)?;
 
@@ -125,7 +138,11 @@ pub fn select_many(prompt: &str, options: &[(&str, bool)]) -> io::Result<Vec<usi
             stdout.flush()?;
 
-            // Read key
+            // Read key — only act on Press events to avoid double-firing
+            // from Release/Repeat events on Windows.
             if let Event::Key(KeyEvent {
-                code, modifiers, ..
+                code,
+                modifiers,
+                kind: KeyEventKind::Press,
+                ..
             }) = event::read()?
             {
@@ -201,17 +218,14 @@ fn read_secret_line() -> io::Result<SecretString> {
     let mut stdout = io::stdout();
 
-    // Drain any residual key events (e.g. Enter from a prior `read_line` prompt)
-    // that are already queued before we start reading. Without this, on
-    // Windows the leftover Enter is immediately consumed and the function
-    // returns an empty string before the user can type anything.
-    // Uses Duration::ZERO so we never block waiting for new input — only
-    // events already in the queue are consumed.
-    while event::poll(std::time::Duration::ZERO)? {
-        let _ = event::read()?;
-    }
+    drain_pending_events();
 
     loop {
+        // Only act on Press events to avoid double-firing from
+        // Release/Repeat events on Windows.
         if let Event::Key(KeyEvent {
-            code, modifiers, ..
+            code,
+            modifiers,
+            kind: KeyEventKind::Press,
+            ..
         }) = event::read()?
         {
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
