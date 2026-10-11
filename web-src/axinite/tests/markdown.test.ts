/**
 * @file Property-based tests (fast-check) for `renderMarkdown`: whatever the
 * input, it may emit only an allow-listed set of attribute-free elements.
 */

import fc from "fast-check";
import { describe, expect, it } from "vitest";

import { renderMarkdown } from "@/lib/markdown";

// The only elements `renderMarkdown` may emit (see markdown.ts), all without
// attributes.
const ALLOWED_ELEMENTS = new Set([
  "p",
  "strong",
  "em",
  "code",
  "h1",
  "h2",
  "h3",
  "h4",
  "h5",
  "h6",
  "ul",
  "ol",
  "li",
  "table",
  "thead",
  "tbody",
  "tr",
  "th",
  "td",
]);

// Fragments weighted towards HTML and markdown metacharacters, so generated
// sources hit the escaping and every block and inline transform.
const FRAGMENT = fc.oneof(
  fc.constantFrom(
    "<",
    ">",
    "&",
    '"',
    "'",
    "`",
    "*",
    "**",
    "_",
    "#",
    "## ",
    "|",
    "| - |",
    "- ",
    "1. ",
    "\n",
    "\n\n",
    "[x](javascript:alert(1))",
    "<script>",
    "</p>",
    "<img src=x onerror=alert(1)>",
    "&lt;b&gt;",
    "javascript:"
  ),
  fc.string({ maxLength: 8 })
);
const SOURCE = fc
  .array(FRAGMENT, { maxLength: 24 })
  .map((fragments) => fragments.join(""));

// The chat view assigns renderMarkdown output to innerHTML, so these tests pin
// the escape-first invariant: every character of the source is HTML-escaped
// before the renderer adds its own fixed, attribute-free tags.
describe("renderMarkdown", () => {
  it.each([
    ["a script element", "<script>alert(1)</script>"],
    ["an event-handler attribute", '<img src=x onerror="alert(1)">'],
    ["a javascript: link", "[x](javascript:alert(1))"],
    ["markup inside a code span", "`<b onclick='x'>`"],
    ["markup inside a table cell", "| a |\n| - |\n| <iframe src=x> |"],
  ])("escapes %s", (_label, source) => {
    const html = renderMarkdown(source);
    // No element the renderer did not create, and no element with attributes.
    expect(html).not.toMatch(/<(script|img|iframe|a)\b/iu);
    expect(html).not.toMatch(/<[a-z][^>]*\s[a-z-]+=/iu);
  });

  it("still renders the supported markdown subset", () => {
    expect(renderMarkdown("**bold** and `code`")).toContain(
      "<strong>bold</strong> and <code>code</code>"
    );
  });

  it("only ever emits allow-listed, attribute-free elements", () => {
    fc.assert(
      fc.property(SOURCE, (source) => {
        const container = document.createElement("div");
        container.innerHTML = renderMarkdown(source);
        for (const element of container.querySelectorAll("*")) {
          expect(ALLOWED_ELEMENTS.has(element.localName)).toBe(true);
          expect(element.attributes.length).toBe(0);
        }
      }),
      { numRuns: 300, seed: 20_261_011 }
    );
  });
});
