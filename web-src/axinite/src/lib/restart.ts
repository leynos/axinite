import type { ChatSseEvent } from "@/lib/api/contracts";

export type RestartPhase = "idle" | "restarting" | "restarted";

/** A successful completion of the `restart` tool call. */
function isRestartToolCompleted(event: ChatSseEvent): boolean {
  return (
    event.type === "tool_completed" && event.name === "restart" && event.success
  );
}

/** A response whose text announces "restart initiated" (case-insensitive). */
function isRestartAnnouncement(event: ChatSseEvent): boolean {
  return (
    event.type === "response" &&
    event.content.toLowerCase().includes("restart initiated")
  );
}

/**
 * Whether a chat event signals that the gateway has accepted the `/restart`
 * command. Mirrors the legacy heuristic: a completed `restart` tool call, or a
 * response whose text contains "restart initiated" (case-insensitive).
 */
export function isRestartInitiated(event: ChatSseEvent): boolean {
  return isRestartToolCompleted(event) || isRestartAnnouncement(event);
}

export type RestartStreamHandlers = {
  onEvent: (event: ChatSseEvent) => void;
  onOpen: () => void;
  onError: () => void;
};

export type RestartStreamHandle = {
  close: () => void;
};

export type RestartDeps = {
  /** POST the `/restart` command through the chat send API. */
  sendRestart: () => Promise<unknown>;
  /** Open a dedicated chat event stream for reconnection detection. */
  openStream: (handlers: RestartStreamHandlers) => RestartStreamHandle;
  /** Resolve `true` when the gateway status endpoint is reachable. */
  checkStatus: () => Promise<boolean>;
  /** Schedule repeated status polls; returns a cancel function. */
  scheduleStatusPolls: (tick: () => void) => () => void;
  /** Report phase transitions to the host component. */
  onPhase: (phase: RestartPhase) => void;
};

export type RestartController = {
  start: () => void;
  dispose: () => void;
};

/**
 * State machine for the gateway restart affordance.
 *
 * The gateway has no restart endpoint; the legacy UI sends `/restart` through
 * chat and treats stream reconnection (or a recovered status poll) as
 * completion. Completion only fires once the gateway is observed to have gone
 * down (a stream error or a failed status poll) and then come back, avoiding a
 * premature "restarted" while the old process is still answering.
 */
export function createRestartController(deps: RestartDeps): RestartController {
  let initiated = false;
  let wentDown = false;
  let finished = false;
  let handle: RestartStreamHandle | undefined;
  let cancelPolls: (() => void) | undefined;

  const complete = () => {
    if (finished) {
      return;
    }
    finished = true;
    cancelPolls?.();
    cancelPolls = undefined;
    handle?.close();
    handle = undefined;
    deps.onPhase("restarted");
  };

  const beginPolling = () => {
    if (cancelPolls) {
      return;
    }
    cancelPolls = deps.scheduleStatusPolls(() => {
      if (finished || !initiated) {
        return;
      }
      void deps
        .checkStatus()
        .then((reachable) => {
          if (finished) {
            return;
          }
          if (!reachable) {
            wentDown = true;
          } else if (wentDown) {
            complete();
          }
        })
        .catch(() => {
          wentDown = true;
        });
    });
  };

  const onEvent = (event: ChatSseEvent) => {
    if (!initiated && isRestartInitiated(event)) {
      initiated = true;
      beginPolling();
    }
  };

  const onError = () => {
    if (initiated) {
      wentDown = true;
    }
  };

  const onOpen = () => {
    if (initiated && wentDown) {
      complete();
    }
  };

  const start = () => {
    if (handle || finished) {
      return;
    }
    deps.onPhase("restarting");
    handle = deps.openStream({ onEvent, onOpen, onError });
    void deps.sendRestart().catch(() => {
      // A failed send aborts the restart; surface idle so the button re-enables.
      cancelPolls?.();
      cancelPolls = undefined;
      handle?.close();
      handle = undefined;
      finished = true;
      deps.onPhase("idle");
    });
  };

  const dispose = () => {
    finished = true;
    cancelPolls?.();
    cancelPolls = undefined;
    handle?.close();
    handle = undefined;
  };

  return { start, dispose };
}
