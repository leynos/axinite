/** @file Preview transport contracts using only injected files, fetch and deadlines. */
import { describe, expect, it, vi } from "vitest";
import {
  createPreviewHandler,
  type PreviewDependencies,
} from "../../mock-backend/src/preview-server";

function harness(files: Record<string, string> = {}) {
  const upstream = vi
    .fn<PreviewDependencies["fetchUpstream"]>()
    .mockResolvedValue(new Response("upstream"));
  const readFile = vi.fn((filePath: string) => files[filePath] ?? "");
  const handler = createPreviewHandler(
    { distDir: "/dist", apiPort: 8787 },
    {
      isFile: (filePath) => Object.hasOwn(files, filePath),
      readFile,
      fetchUpstream: upstream,
    }
  );
  const server = { timeout: vi.fn() };
  const request = (route: string, init?: RequestInit) =>
    handler(new Request(`http://preview.test${route}`, init), server);
  return { upstream, readFile, server, request };
}

describe("preview static routing", () => {
  it.each([
    ["/", { "/dist/index.html": "shell" }, "shell"],
    ["/chat", { "/dist/index.html": "shell" }, "shell"],
    [
      "/chat",
      { "/dist/chat.html": "sibling", "/dist/index.html": "shell" },
      "sibling",
    ],
    [
      "/chat",
      { "/dist/chat/index.html": "directory", "/dist/chat.html": "sibling" },
      "directory",
    ],
    [
      "/chat",
      { "/dist/chat": "exact", "/dist/chat/index.html": "directory" },
      "exact",
    ],
    ["/assets/app.js", { "/dist/assets/app.js": "script" }, "script"],
  ] as const)("serves %s with the expected candidate precedence", async (route, files, content) => {
    const h = harness(files);
    const response = await h.request(route);
    expect(response.status).toBe(200);
    expect(await response.text()).toBe(content);
    expect(h.upstream).not.toHaveBeenCalled();
    expect(h.server.timeout).not.toHaveBeenCalled();
  });

  it.each([
    "/assets/missing.js",
    "/unknown",
  ])("returns static404 for %s without a candidate", async (route) => {
    const h = harness(
      route.includes(".") ? { "/dist/index.html": "shell" } : {}
    );
    const response = await h.request(route);
    expect(response.status).toBe(404);
    expect(await response.text()).toBe("Not found");
    expect(h.readFile).not.toHaveBeenCalled();
  });
});

describe("preview upstream transport", () => {
  it("preserves encoded query, method, request headers/body and response metadata", async () => {
    const h = harness();
    h.upstream.mockImplementation(async (url, init) => {
      expect(url.href).toBe(
        "http://127.0.0.1:8787/api/settings/a%2Fb?deployment=d%20e"
      );
      expect(init.method).toBe("PUT");
      const headers = new Headers(init.headers);
      expect(headers.get("authorization")).toBe("Bearer fixture");
      expect(headers.get("content-type")).toBe("application/json");
      expect(await new Response(init.body).text()).toBe('{"value":false}');
      return new Response("upstream error", {
        status: 409,
        statusText: "Conflict",
        headers: { "x-fixture": "kept" },
      });
    });
    const response = await h.request("/api/settings/a%2Fb?deployment=d%20e", {
      method: "PUT",
      headers: {
        authorization: "Bearer fixture",
        "content-type": "application/json",
      },
      body: '{"value":false}',
    });
    expect(response.status).toBe(409);
    expect(response.statusText).toBe("Conflict");
    expect(response.headers.get("x-fixture")).toBe("kept");
    expect(await response.text()).toBe("upstream error");
  });

  it.each([
    "GET",
    "HEAD",
  ])("forwards %s without a request body and preserves204", async (method) => {
    const h = harness();
    h.upstream.mockResolvedValue(new Response(null, { status: 204 }));
    expect((await h.request("/api/jobs", { method })).status).toBe(204);
    expect(h.upstream.mock.calls[0]?.[1].body).toBeNull();
  });

  it.each([
    "/api/chat/events?token=fixture",
    "/api/logs/events",
  ])("disables deadlines only for SSE %s", async (route) => {
    const h = harness();
    h.upstream.mockResolvedValue(
      new Response("event: heartbeat\ndata: {}\n\n", {
        headers: { "content-type": "text/event-stream" },
      })
    );
    const response = await h.request(route);
    expect(h.server.timeout).toHaveBeenCalledTimes(1);
    expect(h.server.timeout.mock.calls[0]?.[1]).toBe(0);
    expect(response.headers.get("content-type")).toBe("text/event-stream");
    expect(await response.text()).toContain("event: heartbeat");
  });

  it("keeps ordinary API deadlines and propagates upstream failures", async () => {
    const h = harness();
    const failure = new Error("fixture fetch failure");
    h.upstream.mockRejectedValue(failure);
    await expect(h.request("/api/jobs/events")).rejects.toBe(failure);
    expect(h.server.timeout).not.toHaveBeenCalled();
  });
});
