import { afterEach, describe, expect, it, vi } from "vitest";

import { isHttpUrl, openExternalUrl } from "@/components/chat-cards";

describe("isHttpUrl", () => {
  it("accepts absolute http and https URLs", () => {
    expect(isHttpUrl("https://example.test/oauth")).toBe(true);
    expect(isHttpUrl("http://example.test/callback")).toBe(true);
  });

  it("rejects javascript: URLs", () => {
    expect(isHttpUrl("javascript:alert(1)")).toBe(false);
  });

  it("rejects data:, file:, relative, and unparseable values", () => {
    expect(isHttpUrl("data:text/html,<h1>hi</h1>")).toBe(false);
    expect(isHttpUrl("file:///etc/passwd")).toBe(false);
    expect(isHttpUrl("/relative/path")).toBe(false);
    expect(isHttpUrl("not a url")).toBe(false);
    expect(isHttpUrl("")).toBe(false);
  });
});

describe("openExternalUrl", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("opens safe http(s) URLs in a new noopener tab", () => {
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    expect(openExternalUrl("https://example.test/oauth")).toBe(true);
    expect(open).toHaveBeenCalledWith(
      "https://example.test/oauth",
      "_blank",
      "noopener,noreferrer"
    );
  });

  it("refuses to open javascript: URLs", () => {
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    expect(openExternalUrl("javascript:alert(1)")).toBe(false);
    expect(open).not.toHaveBeenCalled();
  });
});
