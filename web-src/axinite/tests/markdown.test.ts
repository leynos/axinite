import { describe, expect, it } from "vitest";

import { renderMarkdown } from "@/lib/markdown";

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
});
