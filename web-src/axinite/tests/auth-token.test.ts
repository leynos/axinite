import { afterEach, describe, expect, it, vi } from "vitest";
import { createEventStream, requestJson } from "@/lib/api/client";
import {
  appendTokenToUrl,
  clearGatewayToken,
  getGatewayToken,
  setGatewayToken,
} from "@/lib/auth/token";

// The gateway protects every /api/* route with a bearer token; the two SSE
// endpoints accept the token as a query parameter because EventSource cannot
// set headers (src/channels/web/auth.rs). The typed client must thread the
// stored token through both transports.

afterEach(() => {
  clearGatewayToken();
  vi.unstubAllGlobals();
});

describe("gateway token storage", () => {
  it("stores, returns, and clears the token", () => {
    expect(getGatewayToken()).toBeNull();
    setGatewayToken("secret-token");
    expect(getGatewayToken()).toBe("secret-token");
    clearGatewayToken();
    expect(getGatewayToken()).toBeNull();
  });

  it("survives module state through sessionStorage", () => {
    setGatewayToken("persisted");
    expect(window.sessionStorage.getItem("axinite.gateway-token")).toBe(
      "persisted"
    );
  });
});

describe("bearer header injection", () => {
  it("sends Authorization when a token is stored", async () => {
    setGatewayToken("secret-token");
    const fetchMock = vi.fn(
      async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response("{}", {
          headers: { "Content-Type": "application/json" },
        })
    );
    vi.stubGlobal("fetch", fetchMock);

    await requestJson("/api/gateway/status");

    const init = fetchMock.mock.calls[0]?.[1];
    const headers = new Headers(init?.headers);
    expect(headers.get("Authorization")).toBe("Bearer secret-token");
  });

  it("omits Authorization when no token is stored", async () => {
    const fetchMock = vi.fn(
      async (_input: RequestInfo | URL, _init?: RequestInit) =>
        new Response("{}", {
          headers: { "Content-Type": "application/json" },
        })
    );
    vi.stubGlobal("fetch", fetchMock);

    await requestJson("/api/gateway/status");

    const init = fetchMock.mock.calls[0]?.[1];
    const headers = new Headers(init?.headers);
    expect(headers.get("Authorization")).toBeNull();
  });
});

describe("SSE token propagation", () => {
  it("appends the token as a query parameter", () => {
    setGatewayToken("sse-token");
    expect(appendTokenToUrl("/api/chat/events")).toBe(
      "/api/chat/events?token=sse-token"
    );
    expect(appendTokenToUrl("/api/logs/events?foo=1")).toBe(
      "/api/logs/events?foo=1&token=sse-token"
    );
  });

  it("leaves the URL unchanged without a token", () => {
    expect(appendTokenToUrl("/api/chat/events")).toBe("/api/chat/events");
  });

  it("threads the token into the EventSource URL", () => {
    setGatewayToken("sse-token");

    class FakeEventSource {
      url: string;
      withCredentials: boolean;
      constructor(url: string, init?: EventSourceInit) {
        this.url = url;
        this.withCredentials = init?.withCredentials ?? false;
      }
      addEventListener() {}
      removeEventListener() {}
      close() {}
    }
    vi.stubGlobal("EventSource", FakeEventSource);

    const stream = createEventStream("/api/chat/events");
    expect(stream.url).toBe("/api/chat/events?token=sse-token");
  });
});

describe("SSE error-event handling", () => {
  it("ignores browser connection-error events that carry no data", async () => {
    const { connectChatEvents } = await import("@/lib/api/chat");

    class FakeEventSource extends EventTarget {
      url: string;
      withCredentials = false;
      constructor(url: string) {
        super();
        this.url = url;
      }
      close() {}
    }
    vi.stubGlobal("EventSource", FakeEventSource);

    const received: unknown[] = [];
    const source = connectChatEvents((event) => received.push(event));

    // Browser-style connection failure: an "error" Event with no data.
    source.dispatchEvent(new Event("error"));
    expect(received).toEqual([]);

    // Gateway-style SSE frame: a MessageEvent with a JSON payload.
    source.dispatchEvent(
      new MessageEvent("error", {
        data: JSON.stringify({ type: "error", message: "boom" }),
      })
    );
    expect(received).toEqual([{ type: "error", message: "boom" }]);
  });
});
