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
  const reader = (response.body as ReadableStream<Uint8Array>).getReader();
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
    expect(decode((await reader.read()).value)).toBe("data: one\n\n");

    subscriber.close();
    subscriber.close();
    // Frames after teardown, including heartbeats, are dropped, not thrown.
    expect(() => subscriber.send("data: late\n\n")).not.toThrow();
    vi.advanceTimersByTime(30_000);

    expect(unsubscribe).toHaveBeenCalledTimes(1);
    expect((await reader.read()).done).toBe(true);
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
