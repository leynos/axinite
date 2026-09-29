import { describe, expect, it, vi } from "vitest";
import { activateExtension, installExtension } from "@/lib/api/extensions";
import { fetchJobDetail, promptJob } from "@/lib/api/jobs";
import { setLogLevel } from "@/lib/api/logs";
import { deleteRoutine } from "@/lib/api/routines";
import { removeSkill } from "@/lib/api/skills";
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

  it("replays the newest 25 entries oldest-to-newest", () => {
    const state = new MockBackendState();
    let newest = "";
    for (let index = 0; index < 30; index += 1) {
      newest = state.createThread().id;
    }
    const received: { message: string; timestamp: string }[] = [];
    state.subscribeToLogs({
      send: (entry) => received.push(entry),
      close: () => undefined,
    })();

    expect(received).toHaveLength(25);
    const timestamps = received.map((entry) => Date.parse(entry.timestamp));
    expect(timestamps).toEqual([...timestamps].sort((a, b) => a - b));
    // The most recent write is the last entry replayed.
    expect(received.at(-1)?.message).toBe(`Created thread ${newest}.`);
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

function stubFetch(status = 200, body: unknown = { success: true }) {
  const fetchMock = vi.fn(
    async (_input: RequestInfo | URL, _init?: RequestInit) =>
      new Response(JSON.stringify(body), {
        status,
        headers: { "Content-Type": "application/json" },
      })
  );
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

describe("log level contract", () => {
  it("sets the level with PUT, the only write method the gateway routes", async () => {
    const fetchMock = stubFetch(200, { level: "debug" });
    try {
      await expect(setLogLevel("debug")).resolves.toEqual({ level: "debug" });
    } finally {
      vi.unstubAllGlobals();
    }

    const [url, init] = fetchMock.mock.calls[0] ?? [];
    expect(url).toBe("/api/logs/level");
    expect(init?.method).toBe("PUT");
    expect(JSON.parse(String(init?.body))).toEqual({ level: "debug" });
  });

  it("propagates a rejected level change instead of reporting success", async () => {
    stubFetch(503, "Log level control not available");
    try {
      await expect(setLogLevel("debug")).rejects.toThrow();
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("resource path contract", () => {
  it.each([
    [
      "extension",
      () => activateExtension("a/b?c#d"),
      "/api/extensions/a%2Fb%3Fc%23d/activate",
    ],
    ["job", () => fetchJobDetail("../x"), "/api/jobs/..%2Fx"],
    ["routine", () => deleteRoutine("r 1"), "/api/routines/r%201"],
    ["skill", () => removeSkill("x/../y"), "/api/skills/x%2F..%2Fy"],
  ] as const)("encodes the %s identifier as one path segment", async (_kind, call, expected) => {
    const fetchMock = stubFetch();
    try {
      await call();
    } finally {
      vi.unstubAllGlobals();
    }
    expect(fetchMock.mock.calls[0]?.[0]).toBe(expected);
  });
});
