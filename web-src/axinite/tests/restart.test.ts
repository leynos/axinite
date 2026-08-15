import { describe, expect, it } from "vitest";

import type { ChatSseEvent } from "@/lib/api/contracts";
import {
  createRestartController,
  isRestartInitiated,
  type RestartPhase,
  type RestartStreamHandlers,
} from "@/lib/restart";

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

function makeHarness(options?: { reachable?: boolean; failSend?: boolean }) {
  const phases: RestartPhase[] = [];
  let handlers: RestartStreamHandlers | undefined;
  let closed = false;
  let reachable = options?.reachable ?? true;
  let pollTick: (() => void) | undefined;

  const controller = createRestartController({
    sendRestart: () =>
      options?.failSend
        ? Promise.reject(new Error("send failed"))
        : Promise.resolve(),
    openStream: (h) => {
      handlers = h;
      return {
        close: () => {
          closed = true;
        },
      };
    },
    checkStatus: () => Promise.resolve(reachable),
    scheduleStatusPolls: (tick) => {
      pollTick = tick;
      return () => {
        pollTick = undefined;
      };
    },
    onPhase: (phase) => phases.push(phase),
  });

  return {
    controller,
    phases,
    get closed() {
      return closed;
    },
    emit: (event: ChatSseEvent) => handlers?.onEvent(event),
    open: () => handlers?.onOpen(),
    error: () => handlers?.onError(),
    setReachable: (value: boolean) => {
      reachable = value;
    },
    poll: async () => {
      pollTick?.();
      await flush();
    },
    hasPoll: () => pollTick !== undefined,
  };
}

const restartToolCompleted: ChatSseEvent = {
  type: "tool_completed",
  name: "restart",
  success: true,
};

describe("isRestartInitiated", () => {
  it("recognizes a completed restart tool call", () => {
    expect(isRestartInitiated(restartToolCompleted)).toBe(true);
  });

  it("recognizes a response mentioning restart initiated (case-insensitive)", () => {
    expect(
      isRestartInitiated({
        type: "response",
        content: "Gateway RESTART INITIATED, hold tight",
        thread_id: "t1",
      })
    ).toBe(true);
  });

  it("ignores unrelated events", () => {
    expect(
      isRestartInitiated({
        type: "tool_completed",
        name: "search",
        success: true,
      })
    ).toBe(false);
    expect(
      isRestartInitiated({
        type: "response",
        content: "all done",
        thread_id: "t",
      })
    ).toBe(false);
  });
});

describe("createRestartController", () => {
  it("completes when the stream reconnects after going down", async () => {
    const h = makeHarness();
    h.controller.start();
    expect(h.phases).toEqual(["restarting"]);

    h.emit(restartToolCompleted);
    expect(h.hasPoll()).toBe(true);
    h.error();
    h.open();

    expect(h.phases).toEqual(["restarting", "restarted"]);
    expect(h.closed).toBe(true);
  });

  it("completes via a recovered status poll after a failed poll", async () => {
    const h = makeHarness();
    h.controller.start();
    h.emit({
      type: "response",
      content: "restart initiated",
      thread_id: "t",
    });

    h.setReachable(false);
    await h.poll();
    expect(h.phases).toEqual(["restarting"]);

    h.setReachable(true);
    await h.poll();
    expect(h.phases).toEqual(["restarting", "restarted"]);
  });

  it("does not complete prematurely while the gateway is still up", async () => {
    const h = makeHarness();
    h.controller.start();

    // Reconnect signal before any restart is initiated must be ignored.
    h.open();
    h.emit(restartToolCompleted);

    // A reachable poll without an intervening outage must not complete.
    await h.poll();
    expect(h.phases).toEqual(["restarting"]);
  });

  it("returns to idle when the restart command fails to send", async () => {
    const h = makeHarness({ failSend: true });
    h.controller.start();
    await flush();

    expect(h.phases).toEqual(["restarting", "idle"]);
    expect(h.closed).toBe(true);
  });
});
