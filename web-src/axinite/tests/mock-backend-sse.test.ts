/**
 * @file Tests for the mock backend's `buildSseResponse`: subscription, heartbeat
 * and teardown of a stream, driven with fake timers.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { buildSseResponse } from "../../mock-backend/src/server";

type Subscriber = { send: (frame: string) => void; close: () => void };

function harness() {
  let subscriber: Subscriber | undefined;
  const unsubscribe = vi.fn();
  const response = buildSseResponse({
    subscribe: (next) => {
      subscriber = next;
      return unsubscribe;
    },
    heartbeatFrame: ": keep-alive\n\n",
  });
  const stream = response.body;
  if (!stream) {
    throw new Error("stream response had no body");
  }
  const reader = stream.getReader();
  if (!subscriber) {
    throw new Error("stream did not subscribe");
  }
  return { subscriber, unsubscribe, reader };
}

const decode = (chunk: Uint8Array | undefined) =>
  new TextDecoder().decode(chunk);

describe("mock backend SSE teardown", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("tears down once when the state closes the subscriber", async () => {
    const { subscriber, unsubscribe, reader } = harness();
    subscriber.send("data: one\n\n");
    const firstRead = await reader.read();
    expect(decode(firstRead.value)).toBe("data: one\n\n");

    subscriber.close();
    subscriber.close();
    // Frames after teardown, including heartbeats, are dropped, not thrown.
    expect(() => subscriber.send("data: late\n\n")).not.toThrow();
    vi.advanceTimersByTime(30_000);

    expect(unsubscribe).toHaveBeenCalledTimes(1);
    const finalRead = await reader.read();
    expect(finalRead.done).toBe(true);
  });

  it("tears down once when the client cancels", async () => {
    const { subscriber, unsubscribe, reader } = harness();
    await reader.cancel();
    subscriber.close();

    expect(unsubscribe).toHaveBeenCalledTimes(1);
    expect(() => subscriber.send("data: late\n\n")).not.toThrow();
    expect(vi.getTimerCount()).toBe(0);
  });
});
