# Replace regex HTML sanitizer with DOMPurify to prevent XSS

## Summary

- Source commit: `28a22f2a59239df9ea6efd7430a9491b960471c5`
- Source date: `2026-03-11`
- Severity: `critical`
- Main relevance: `yes`
- Effectiveness: `strong`
- Scope and blast radius: narrow web gateway HTML rendering path. Replaces a
  regex sanitizer with DOMPurify and closes a prompt-injection-to-XSS chain in
  the browser UI.

## What the upstream commit addressed

Upstream commit `28a22f2a59239df9ea6efd7430a9491b960471c5` (`fix(security):
replace regex HTML sanitizer with DOMPurify to prevent XSS (#510)`) addresses
replace regex html sanitizer with dompurify to prevent xss.

Changed upstream paths:

- src/channels/web/static/app.js
- src/channels/web/static/index.html

Upstream stats:

```text
 src/channels/web/static/app.js     | 32 +++++++++++++-------------------
 src/channels/web/static/index.html |  5 +++++
 2 files changed, 18 insertions(+), 19 deletions(-)
```

## Why this is still relevant to Axinite

The audit marked this as `main: yes` with `critical` severity. That keeps the
underlying failure mode close enough to Axinite's current runtime to justify a
follow-up review. The recorded blast radius upstream was narrow web gateway HTML
rendering path. Replaces a regex sanitizer with DOMPurify and closes a
prompt-injection-to-XSS chain in the browser UI.

## Compatibility concerns

The upstream approach looks architecturally portable, but Axinite still needs a
local verification pass before any code is lifted across.

## Risks and benefits of the fix

- Benefits: addresses a `critical`-class issue, gives Axinite an upstream repair
  shape to compare against, and comes with `strong` effectiveness in the staging
  audit.
- Risks: the upstream patch may rely on NearAI-specific assumptions, and the
  recorded blast radius (narrow web gateway HTML rendering path. Replaces a
  regex sanitizer with DOMPurify and closes a prompt-injection-to-XSS chain in
  the browser UI) means the fix could touch more behaviour than the narrow
  symptom suggests.

## Relevant upstream diff

The upstream patch is small and targeted enough to include directly in the issue
draft for implementation reference.

```diff
diff --git a/src/channels/web/static/app.js b/src/channels/web/static/app.js
index 7ca9a25b..de1f83b6 100644
--- a/src/channels/web/static/app.js
+++ b/src/channels/web/static/app.js
@@ -677,24 +677,18 @@ function renderMarkdown(text) {
 }
 
-// Strip dangerous HTML elements and attributes from rendered markdown.
-// This prevents XSS from tool output or prompt injection in LLM responses.
+// Sanitize rendered HTML using DOMPurify to prevent XSS from tool output
+// or prompt injection in LLM responses. DOMPurify is a DOM-based sanitizer
+// that handles all known bypass vectors (SVG onload, newline-split event
+// handlers, mutation XSS, etc.) unlike the regex approach it replaces.
 function sanitizeRenderedHtml(html) {
-  html = html.replace(/<script\b[^<]*(?:(?!<\/script>)<[^<]*)*<\/script>/gi, '');
-  html = html.replace(/<iframe\b[^>]*>[\s\S]*?<\/iframe>/gi, '');
-  html = html.replace(/<object\b[^>]*>[\s\S]*?<\/object>/gi, '');
-  html = html.replace(/<embed\b[^>]*\/?>/gi, '');
-  html = html.replace(/<form\b[^>]*>[\s\S]*?<\/form>/gi, '');
-  html = html.replace(/<style\b[^>]*>[\s\S]*?<\/style>/gi, '');
-  html = html.replace(/<link\b[^>]*\/?>/gi, '');
-  html = html.replace(/<base\b[^>]*\/?>/gi, '');
-  html = html.replace(/<meta\b[^>]*\/?>/gi, '');
-  // Remove event handler attributes (onclick, onerror, onload, etc.)
-  html = html.replace(/\s+on\w+\s*=\s*"[^"]*"/gi, '');
-  html = html.replace(/\s+on\w+\s*=\s*'[^']*'/gi, '');
-  html = html.replace(/\s+on\w+\s*=\s*[^\s>]+/gi, '');
-  // Remove javascript: and data: URLs in href/src attributes
-  html = html.replace(/(href|src|action)\s*=\s*["']?\s*javascript\s*:/gi, '$1="');
-  html = html.replace(/(href|src|action)\s*=\s*["']?\s*data\s*:/gi, '$1="');
-  return html;
+  if (typeof DOMPurify !== 'undefined') {
+    return DOMPurify.sanitize(html, {
+      USE_PROFILES: { html: true },
+      FORBID_TAGS: ['style', 'script'],
+      FORBID_ATTR: ['style', 'onerror', 'onload']
+    });
+  }
+  // DOMPurify not available (CDN unreachable) — return empty string rather than unsanitized HTML
+  return '';
 }
 
diff --git a/src/channels/web/static/index.html b/src/channels/web/static/index.html
index 6f21b428..b6dd9d3a 100644
--- a/src/channels/web/static/index.html
+++ b/src/channels/web/static/index.html
@@ -16,4 +16,9 @@
   <script src="/i18n/zh-CN.js"></script>
   
+  <script
+    src="https://cdnjs.cloudflare.com/ajax/libs/dompurify/3.2.3/purify.min.js"
+    integrity="sha384-osZDKVu4ipZP703HmPOhWdyBajcFyjX2Psjk//TG1Rc0AdwEtuToaylrmcK3LdAl"
+    crossorigin="anonymous"
+  ></script>
   <script
     src="https://cdn.jsdelivr.net/npm/marked@17.0.2/lib/marked.umd.min.js"
```

## Suggested follow-up

1. Verify whether Axinite already blocks or mitigates this failure mode at the
   corresponding seam.
2. If the issue is still live, adapt the upstream fix instead of assuming the
   patch applies unchanged.
3. Add or update regression coverage before shipping any implementation.
