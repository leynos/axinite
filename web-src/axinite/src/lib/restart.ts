/**
 * @file Gateway restart state machine, kept free of Solid and browser globals so
 * it can be tested with injected dependencies. `restart-control.tsx` supplies
 * the live wiring to the chat API, chat event stream, and status polling.
 */

import type { ChatSseEvent } from "@/lib/api/contracts";

export type RestartPhase = "idle" | "restarting" | "restarted" | "failed";

/** Bounded operational categories; no response bodies or URLs cross this port. */
export type RestartFailureCategory = "timeout" | "transport" | "http";

export type RestartSendResult =
  | { kind: "accepted" }
  | { kind: "failed"; category: RestartFailureCategory };

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
  /** POST `/restart`; return a bounded result from the chat API adapter. */
  sendRestart: () => Promise<RestartSendResult>;
  /** Open a dedicated chat event stream for reconnection detection. */
  openStream: (handlers: RestartStreamHandlers) => RestartStreamHandle;
  /** Resolve `true` when the gateway status endpoint is reachable. */
  checkStatus: () => Promise<boolean>;
  /** Schedule repeated status polls; returns a cancel function. */
  scheduleStatusPolls: (tick: () => void) => () => void;
  /** Report phase transitions to the host component. */
  onPhase: (phase: RestartPhase) => void;
  /** Reports a retryable command failure without leaking transport details. */
  onFailure?: (category: RestartFailureCategory) => void;
};

export type RestartController = {
  start: () => void;
  dispose: () => void;
};

type RestartAttempt = {
  initiated: boolean;
  wentDown: boolean;
  handle: RestartStreamHandle | undefined;
  cancelPolls: (() => void) | undefined;
};

// Attempt identity fences callbacks from closed streams and outstanding promises.
// Retiring an attempt happens before adapter cleanup, which can itself deliver
// callbacks synchronously.
class RestartLifecycle {
  private active: RestartAttempt | undefined;
  private isClosed = false;

  constructor(private readonly deps: RestartDeps) {}

  private isCurrent(attempt: RestartAttempt): boolean {
    return !this.isClosed && this.active === attempt;
  }

  private retire(attempt: RestartAttempt): void {
    this.active = undefined;
    attempt.cancelPolls?.();
    attempt.cancelPolls = undefined;
    attempt.handle?.close();
    attempt.handle = undefined;
  }

  private complete(attempt: RestartAttempt): void {
    if (!this.isCurrent(attempt)) return;
    this.isClosed = true;
    this.retire(attempt);
    this.deps.onPhase("restarted");
  }

  private fail(
    attempt: RestartAttempt,
    category: RestartFailureCategory
  ): void {
    if (!this.isCurrent(attempt)) return;
    this.retire(attempt);
    this.deps.onPhase("failed");
    this.deps.onFailure?.(category);
  }

  private async poll(attempt: RestartAttempt): Promise<void> {
    if (!this.isCurrent(attempt)) return;
    try {
      const reachable = await this.deps.checkStatus();
      if (!this.isCurrent(attempt)) return;
      if (!reachable) {
        attempt.wentDown = true;
      } else if (attempt.wentDown) {
        this.complete(attempt);
      }
    } catch {
      if (this.isCurrent(attempt)) attempt.wentDown = true;
    }
  }

  private beginPolling(attempt: RestartAttempt): void {
    if (attempt.cancelPolls) return;
    attempt.cancelPolls = this.deps.scheduleStatusPolls(() => {
      void this.poll(attempt);
    });
  }

  private acknowledge(attempt: RestartAttempt, event: ChatSseEvent): void {
    if (!this.isCurrent(attempt) || attempt.initiated) return;
    if (isRestartInitiated(event)) {
      attempt.initiated = true;
      this.beginPolling(attempt);
    }
  }

  private observeStreamError(attempt: RestartAttempt): void {
    if (this.isCurrent(attempt) && attempt.initiated) attempt.wentDown = true;
  }

  private observeStreamOpen(attempt: RestartAttempt): void {
    if (!this.isCurrent(attempt) || !attempt.initiated) return;
    if (attempt.wentDown) this.complete(attempt);
  }

  private openStream(attempt: RestartAttempt): void {
    const handle = this.deps.openStream({
      onEvent: (event) => this.acknowledge(attempt, event),
      onError: () => this.observeStreamError(attempt),
      onOpen: () => this.observeStreamOpen(attempt),
    });
    if (this.isCurrent(attempt)) {
      attempt.handle = handle;
    } else {
      handle.close();
    }
  }

  start(): void {
    if (this.active || this.isClosed) return;
    const attempt: RestartAttempt = {
      initiated: false,
      wentDown: false,
      handle: undefined,
      cancelPolls: undefined,
    };
    this.active = attempt;
    this.deps.onPhase("restarting");
    if (!this.isCurrent(attempt)) return;
    try {
      this.openStream(attempt);
      if (this.isCurrent(attempt)) {
        void this.deps.sendRestart().then(
          (result) => {
            if (result.kind === "failed") {
              this.fail(attempt, result.category);
            }
          },
          () => this.fail(attempt, "transport")
        );
      }
    } catch {
      this.fail(attempt, "transport");
    }
  }

  dispose(): void {
    this.isClosed = true;
    if (this.active) this.retire(this.active);
  }
}

/**
 * Runs a restart attempt until an acknowledged command is followed by an
 * observed outage and recovery. Failed sends expose a retryable failed phase.
 * Repeated starts share the active attempt; stale callbacks after failure,
 * completion or disposal cannot publish phases or schedule more work.
 */
export function createRestartController(deps: RestartDeps): RestartController {
  const lifecycle = new RestartLifecycle(deps);
  return { start: () => lifecycle.start(), dispose: () => lifecycle.dispose() };
}
