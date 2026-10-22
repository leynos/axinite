/**
 * @file Streaming transport contracts: gateway-named SSE events, authenticated
 * EventSource URLs, malformed frames and connection failures. The gateway
 * permits query token auth only on streaming paths (web/auth.rs).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { connectChatEvents } from "@/lib/api/chat";
import { createEventStream } from "@/lib/api/client";
import type { ChatSseEvent } from "@/lib/api/contracts";
import { connectLogEvents } from "@/lib/api/logs";
import { clearGatewayToken, setGatewayToken } from "@/lib/auth/token";

class WireEventSource extends EventTarget {
  static latest: WireEventSource;
  readonly close = vi.fn();
  onerror: ((event: Event) => void) | null = null;
  constructor(
    readonly url: string,
    readonly options?: EventSourceInit
  ) {
    super();
    WireEventSource.latest = this;
  }
  frame(name: string, data: string) {
    this.dispatchEvent(new MessageEvent(name, { data }));
  }
  failConnection() {
    const event = new Event("error");
    this.dispatchEvent(event);
    // EventTarget dispatch already invokes the onerror event-handler attribute.
  }
}

beforeEach(() => {
  setGatewayToken("stream +/?#");
  vi.stubGlobal("EventSource", WireEventSource);
});
afterEach(() => {
  clearGatewayToken();
  vi.unstubAllGlobals();
});

// The event names and representative fields come from types/events.rs and
// the SSE publisher in channels/web/sse.rs. These examples cover every named
// browser event, including the gateway's error frame versus a network error.
const frames = [
  { type: "response", content: "reply", thread_id: "thread" },
  { type: "thinking", message: "thinking" },
  { type: "tool_started", name: "read" },
  { type: "tool_completed", name: "read", success: true },
  { type: "tool_result", name: "read", preview: "notes" },
  { type: "stream_chunk", content: "chunk" },
  { type: "status", message: "ready" },
  {
    type: "approval_needed",
    request_id: "request",
    tool_name: "write",
    description: "write notes",
    parameters: "{}",
  },
  {
    type: "auth_required",
    extension_name: "github",
    instructions: "Enter token",
  },
  {
    type: "auth_completed",
    extension_name: "github",
    success: true,
    message: "ready",
  },
  { type: "error", message: "Gateway error" },
  { type: "heartbeat" },
  { type: "extension_status", extension_name: "github", status: "active" },
  {
    type: "job_started",
    job_id: "job",
    title: "Research",
    browse_url: "/projects/job/",
  },
  { type: "job_message", job_id: "job", role: "assistant", content: "working" },
  { type: "job_tool_use", job_id: "job", tool_name: "read", input: {} },
  {
    type: "job_tool_result",
    job_id: "job",
    tool_name: "read",
    output: "notes",
  },
  { type: "job_status", job_id: "job", message: "running" },
  { type: "job_result", job_id: "job", status: "completed" },
  {
    type: "image_generated",
    data_url: "data:image/png;base64,AA==",
    path: "image.png",
  },
];

describe("chat SSE wire contract", () => {
  it.each(frames)("delivers the named $type frame exactly once", (frame) => {
    const listener = vi.fn();
    const error = vi.fn();
    const source = connectChatEvents(listener, error);
    expect(WireEventSource.latest.url).toBe(
      "/api/chat/events?token=stream%20%2B%2F%3F%23"
    );
    expect(WireEventSource.latest.options).toEqual({ withCredentials: false });
    WireEventSource.latest.frame(frame.type, JSON.stringify(frame));
    expect(listener).toHaveBeenCalledExactlyOnceWith(frame);
    expect(error).not.toHaveBeenCalled();
    source.close();
    expect(WireEventSource.latest.close).toHaveBeenCalledTimes(1);
  });

  it("reports malformed frames and connection errors without delivering them", () => {
    const delivered: ChatSseEvent[] = [];
    const errors: string[] = [];
    const listener = (event: ChatSseEvent) => delivered.push(event);
    const error = () => errors.push("stream error");
    connectChatEvents(listener, error);
    WireEventSource.latest.frame("response", "{invalid");
    expect(errors).toHaveLength(1);
    WireEventSource.latest.failConnection();
    expect(errors).toHaveLength(2);
    expect(delivered).toEqual([]);
    const validFrame = frames[0];
    WireEventSource.latest.frame("response", JSON.stringify(validFrame));
    expect(delivered).toEqual([validFrame]);
  });

  it("ignores unrelated event names and tolerates missing error callbacks", () => {
    const listener = vi.fn();
    connectChatEvents(listener);
    WireEventSource.latest.frame("unrecognized", "{}");
    WireEventSource.latest.frame("response", "broken");
    expect(() => WireEventSource.latest.failConnection()).not.toThrow();
    expect(listener).not.toHaveBeenCalled();
  });
});

describe("log SSE wire contract", () => {
  it("delivers log entries but ignores unnamed messages", () => {
    const listener = vi.fn();
    const error = vi.fn();
    const source = connectLogEvents(listener, error);
    expect(WireEventSource.latest.url).toBe(
      "/api/logs/events?token=stream%20%2B%2F%3F%23"
    );
    expect(WireEventSource.latest.options).toEqual({ withCredentials: false });
    const entry = {
      level: "info",
      target: "axinite",
      message: "ready",
      timestamp: "2026-10-01T00:00:00Z",
    };
    WireEventSource.latest.frame("message", JSON.stringify(entry));
    expect(listener).not.toHaveBeenCalled();
    WireEventSource.latest.frame("log", JSON.stringify(entry));
    expect(listener).toHaveBeenCalledExactlyOnceWith(entry);
    expect(error).not.toHaveBeenCalled();
    source.close();
    expect(WireEventSource.latest.close).toHaveBeenCalledTimes(1);
  });

  it("reports malformed log frames and connection errors and then recovers", () => {
    const listener = vi.fn();
    const error = vi.fn();
    connectLogEvents(listener, error);
    WireEventSource.latest.frame("log", "broken");
    WireEventSource.latest.failConnection();
    expect(error).toHaveBeenCalledTimes(2);
    expect(listener).not.toHaveBeenCalled();
    const recovered = {
      level: "info",
      target: "gateway",
      message: "recovered",
      timestamp: "2026-10-01T00:00:00Z",
    };
    WireEventSource.latest.frame("log", JSON.stringify(recovered));
    expect(listener).toHaveBeenCalledExactlyOnceWith(recovered);
  });

  it("tolerates malformed data when no error callback is supplied", () => {
    const listener = vi.fn();
    connectLogEvents(listener);
    expect(() => WireEventSource.latest.frame("log", "broken")).not.toThrow();
    expect(() => WireEventSource.latest.failConnection()).not.toThrow();
    expect(listener).not.toHaveBeenCalled();
  });
});

it("creates an inert stream when EventSource is unavailable", () => {
  Reflect.deleteProperty(globalThis, "EventSource");
  const source = createEventStream("/api/chat/events?existing=1");
  expect(source.url).toBe(
    "/api/chat/events?existing=1&token=stream%20%2B%2F%3F%23"
  );
  expect(() => source.close()).not.toThrow();
});

it("omits query authentication when there is no session token", () => {
  clearGatewayToken();
  createEventStream("/api/logs/events");
  expect(WireEventSource.latest.url).toBe("/api/logs/events");
});
