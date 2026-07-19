import { describe, expect, it, vi } from "vitest";
import { installExtension } from "@/lib/api/extensions";
import { promptJob } from "@/lib/api/jobs";
import { MockBackendState } from "../../mock-backend/src/state";

// These tests pin the browser contract to the real gateway payload shapes
// (see docs/solidjs-pwa-gap-analysis.md §5.1, §5.2, and §11.1). The mock
// backend and typed client must speak the daemon's dialect, not their own.

describe("log entry contract", () => {
  it("emits gateway-shaped log entries with a target field", () => {
    const state = new MockBackendState();
    const received: unknown[] = [];
    const unsubscribe = state.subscribeToLogs({
      send: (entry) => received.push(entry),
      close: () => undefined,
    });
    unsubscribe();

    expect(received.length).toBeGreaterThan(0);
    for (const entry of received) {
      expect(entry).toMatchObject({
        level: expect.any(String),
        target: expect.any(String),
        message: expect.any(String),
        timestamp: expect.any(String),
      });
      expect(entry).not.toHaveProperty("source");
    }
  });
});

describe("job prompt contract", () => {
  it("accepts the gateway request body of content plus done", () => {
    const state = new MockBackendState();
    const response = state.promptJob("job-comparison", {
      content: "Continue with the follow-up",
      done: false,
    });

    expect(response.success).toBe(true);
    const { events } = state.getJobEvents("job-comparison");
    expect(
      events.some((event) =>
        event.message.includes("Continue with the follow-up")
      )
    ).toBe(true);
  });

  it("posts content and done from the typed client", async () => {
    const fetchMock = vi.fn(
      async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response(JSON.stringify({ success: true, message: "ok" }), {
          headers: { "Content-Type": "application/json" },
        })
    );
    vi.stubGlobal("fetch", fetchMock);
    try {
      await promptJob("job-1", { content: "hello", done: true });
    } finally {
      vi.unstubAllGlobals();
    }

    const init = fetchMock.mock.calls[0]?.[1];
    expect(JSON.parse(String(init?.body))).toEqual({
      content: "hello",
      done: true,
    });
  });
});

describe("extension install contract", () => {
  it("posts name, url, and kind from the typed client", async () => {
    const fetchMock = vi.fn(
      async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response(JSON.stringify({ success: true, message: "ok" }), {
          headers: { "Content-Type": "application/json" },
        })
    );
    vi.stubGlobal("fetch", fetchMock);
    try {
      await installExtension({
        name: "github",
        url: "https://example.test/mcp",
        kind: "mcp",
      });
    } finally {
      vi.unstubAllGlobals();
    }

    const init = fetchMock.mock.calls[0]?.[1];
    expect(JSON.parse(String(init?.body))).toEqual({
      name: "github",
      url: "https://example.test/mcp",
      kind: "mcp",
    });
  });
});
