/**
 * @file Every HTTP API wrapper crosses the real client boundary. Expected
 * routes, verbs and bodies are defined against the Rust gateway contract in
 * support/api-wire-cases, rather than replacing requestJson with a mock.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  deleteJson,
  postJson,
  putJson,
  REQUEST_TIMEOUT_MS,
  requestJson,
  withRequestTimeout,
} from "@/lib/api/client";
import {
  deriveGatewayStatus,
  fetchRuntimeFeatureFlags,
} from "@/lib/api/gateway";
import { clearGatewayToken, setGatewayToken } from "@/lib/auth/token";
import { wireCases } from "./support/api-wire-cases";

beforeEach(() => setGatewayToken("wire +/?secret"));
afterEach(() => {
  clearGatewayToken();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

/** Decode a captured fetch call into the wire envelope asserted by every case. */
function requestDetails(
  call:
    | readonly [url: RequestInfo | URL, init?: RequestInit | undefined]
    | undefined
) {
  if (!call) throw new Error("Expected a captured gateway request");
  const [url, init = {}] = call;
  return {
    url,
    method: init.method ?? "GET",
    headers: new Headers(init.headers),
    body: init.body == null ? null : JSON.parse(String(init.body)),
    signal: init.signal,
  };
}

describe("HTTP wrapper wire contracts", () => {
  it.each(
    wireCases
  )("$name sends the exact gateway request and decodes JSON", async (entry) => {
    const payload = entry.response ?? { wire: true };
    const fetchMock = vi.fn(
      async (_url: RequestInfo | URL, _init?: RequestInit) =>
        new Response(JSON.stringify(payload))
    );
    vi.stubGlobal("fetch", fetchMock);
    await expect(entry.call()).resolves.toEqual(entry.result ?? payload);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const actual = requestDetails(fetchMock.mock.calls[0]);
    expect(actual.url).toBe(entry.path);
    expect(actual.method).toBe(entry.method ?? "GET");
    expect(actual.headers.get("Authorization")).toBe("Bearer wire +/?secret");
    expect(actual.headers.get("Accept")).toBe("application/json");
    const jsonWrite = entry.method === "POST" || entry.method === "PUT";
    expect(actual.headers.get("Content-Type")).toBe(
      jsonWrite ? "application/json" : null
    );
    expect(actual.body).toEqual(entry.body ?? null);
    expect(actual.signal).toBeInstanceOf(AbortSignal);
    expect(actual.signal?.aborted).toBe(false);
  });

  it.each(
    wireCases
  )("$name propagates HTTP errors or its documented status fallback", async (entry) => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () => new Response("Gateway rejected request", { status: 429 })
      )
    );
    if ("fallback" in entry) {
      await expect(entry.call()).resolves.toEqual(entry.fallback);
    } else {
      await expect(entry.call()).rejects.toMatchObject({
        category: "http",
        message: "Gateway rejected request",
      });
    }
  });

  it.each(
    wireCases
  )("$name propagates transport errors or its documented status fallback", async (entry) => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        throw new TypeError("Network unreachable");
      })
    );
    if ("fallback" in entry) {
      await expect(entry.call()).resolves.toEqual(entry.fallback);
    } else {
      await expect(entry.call()).rejects.toMatchObject({
        category: "transport",
      });
    }
  });

  it.each(
    wireCases
  )("$name aborts a hung fetch at the request deadline", async (entry) => {
    vi.useFakeTimers();
    let signal: AbortSignal | null | undefined;
    vi.stubGlobal(
      "fetch",
      vi.fn((_url: RequestInfo | URL, init?: RequestInit) => {
        signal = init?.signal;
        return new Promise<Response>((_resolve, reject) =>
          signal?.addEventListener(
            "abort",
            () => reject(new DOMException("Aborted", "AbortError")),
            { once: true }
          )
        );
      })
    );
    const pending = entry.call();
    // Attach the assertion before advancing timers so rejection is observed.
    const assertion =
      "fallback" in entry
        ? expect(pending).resolves.toEqual(entry.fallback)
        : expect(pending).rejects.toMatchObject({ category: "timeout" });
    await vi.advanceTimersByTimeAsync(REQUEST_TIMEOUT_MS - 1);
    expect(signal?.aborted).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(signal?.aborted).toBe(true);
    await assertion;
  });
});

describe("shared HTTP helper contracts", () => {
  it.each([
    ["requestJson", () => requestJson("/empty")],
    ["postJson", () => postJson("/empty")],
    ["putJson", () => putJson("/empty")],
    ["deleteJson", () => deleteJson("/empty")],
  ] as const)("%s accepts an empty 204 response", async (_name, call) => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(null, { status: 204 }))
    );
    await expect(call()).resolves.toBeUndefined();
  });

  it("omits bearer auth when the session has no token", async () => {
    clearGatewayToken();
    const fetchMock = vi.fn(
      async (_url: RequestInfo | URL, _init?: RequestInit) => new Response("{}")
    );
    vi.stubGlobal("fetch", fetchMock);
    await requestJson("/api/jobs");
    expect(
      new Headers(fetchMock.mock.calls[0]?.[1]?.headers).has("Authorization")
    ).toBe(false);
  });

  const headerForms: [
    string,
    (values: Record<string, string>) => HeadersInit,
  ][] = [
    ["plain object", (values) => values],
    ["tuple array", (values) => Object.entries(values)],
    ["native Headers", (values) => new Headers(values)],
  ];

  it.each(
    headerForms
  )("preserves %s headers and JSON defaults for POST and PUT", async (_name, makeHeaders) => {
    const payload = { accepted: true };
    const fetchMock = vi.fn(
      async (_url: RequestInfo | URL, _init?: RequestInit) =>
        new Response(JSON.stringify(payload), { status: 201 })
    );
    vi.stubGlobal("fetch", fetchMock);
    for (const write of [postJson, putJson]) {
      await expect(
        write(
          "/resource",
          { enabled: false },
          makeHeaders({ "X-Deployment-Id": "preview" })
        )
      ).resolves.toEqual(payload);
    }
    expect(fetchMock).toHaveBeenCalledTimes(2);
    for (const [index, [url, init]] of fetchMock.mock.calls.entries()) {
      expect(url).toBe("/resource");
      expect(init?.method).toBe(index === 0 ? "POST" : "PUT");
      const headers = new Headers(init?.headers);
      expect(headers.get("X-Deployment-Id")).toBe("preview");
      expect(headers.get("Content-Type")).toBe("application/json");
      expect(headers.get("Accept")).toBe("application/json");
      expect(headers.get("Authorization")).toBe("Bearer wire +/?secret");
      expect(init?.body).toBe('{"enabled":false}');
    }
  });

  it.each(
    headerForms
  )("preserves explicit %s overrides for POST and PUT", async (_name, makeHeaders) => {
    const payload = { accepted: true };
    const fetchMock = vi.fn(
      async (_url: RequestInfo | URL, _init?: RequestInit) =>
        new Response(JSON.stringify(payload), { status: 202 })
    );
    vi.stubGlobal("fetch", fetchMock);
    const supplied = {
      "x-deployment-id": "tenant_2",
      "content-type": "application/vnd.axinite+json",
      accept: "application/vnd.axinite+json",
      authorization: "Bearer explicit-token",
    };
    for (const write of [postJson, putJson]) {
      await expect(
        write("/resource", { enabled: false }, makeHeaders(supplied))
      ).resolves.toEqual(payload);
    }
    expect(fetchMock).toHaveBeenCalledTimes(2);
    for (const [index, [url, init]] of fetchMock.mock.calls.entries()) {
      expect(url).toBe("/resource");
      expect(init?.method).toBe(index === 0 ? "POST" : "PUT");
      const headers = new Headers(init?.headers);
      for (const [name, value] of Object.entries(supplied)) {
        expect(headers.get(name)).toBe(value);
      }
      expect(init?.body).toBe('{"enabled":false}');
    }
  });

  it("uses an HTTP diagnostic for an empty rejection body", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(null, { status: 503 }))
    );
    await expect(requestJson("/empty-error")).rejects.toMatchObject({
      category: "http",
      message: expect.stringContaining("503"),
    });
  });

  it("releases the request timer after a successful operation", async () => {
    vi.useFakeTimers();
    await expect(withRequestTimeout(async () => "done", 10)).resolves.toBe(
      "done"
    );
    expect(vi.getTimerCount()).toBe(0);
  });

  it("filters non-boolean feature payloads and accepts the flags envelope", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          new Response(
            JSON.stringify({ flags: { route_jobs: false, unknown: 9 } })
          )
      )
    );
    await expect(fetchRuntimeFeatureFlags()).resolves.toEqual({
      route_jobs: false,
    });
  });

  it("derives connected singular and disconnected status pills", () => {
    expect(
      deriveGatewayStatus({
        version: "test",
        total_connections: 1,
        sse_connections: 1,
        ws_connections: 0,
        uptime_secs: 0,
        restart_enabled: false,
      })
    ).toEqual({ label: "Connected", detail: "vtest · 1 live browser stream" });
    expect(deriveGatewayStatus(null)).toEqual({
      label: "Preview",
      detail: "Mock gateway unavailable",
    });
  });
});
