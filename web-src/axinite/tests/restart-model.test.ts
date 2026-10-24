/**
 * @file Bounded adversarial restart histories compared with an observation-led
 * oracle. Deferred sends and polls permit reordered completion; retained stream
 * and polling callbacks deliberately exercise retired attempts.
 */
import fc from "fast-check";
import { describe, expect, it } from "vitest";
import {
  createRestartController,
  type RestartPhase,
  type RestartSendResult,
  type RestartStreamHandlers,
} from "@/lib/restart";
import { ObservationModel } from "./support/restart-observation-model";

type CommandKind =
  | "start"
  | "ack"
  | "open"
  | "error"
  | "poll"
  | "reachable"
  | "unreachable"
  | "poll-error"
  | "send-success"
  | "send-failure"
  | "dispose";
type Command = { kind: CommandKind; target: number };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (cause: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

type Pending<T> = ReturnType<typeof deferred<T>> & {
  owner: number;
  settled: boolean;
};

function latest<T>(items: T[], target: number): T | undefined {
  return items.length > 0
    ? items[items.length - 1 - (target % items.length)]
    : undefined;
}

function harness() {
  const streams: RestartStreamHandlers[] = [];
  const sends: Pending<RestartSendResult>[] = [];
  const polls: Pending<boolean>[] = [];
  const ticks: { owner: number; tick: () => void; cancelled: boolean }[] = [];
  const phases: RestartPhase[] = [];
  const failures: string[] = [];
  const streamClosed: boolean[] = [];
  let pollOwner = -1;
  const controller = createRestartController({
    openStream: (handlers) => {
      const owner = streams.length;
      streams.push(handlers);
      streamClosed.push(false);
      return {
        close: () => {
          streamClosed[owner] = true;
        },
      };
    },
    sendRestart: () => {
      const send = {
        ...deferred<RestartSendResult>(),
        owner: streams.length - 1,
        settled: false,
      };
      sends.push(send);
      return send.promise;
    },
    checkStatus: () => {
      const poll = { ...deferred<boolean>(), owner: pollOwner, settled: false };
      polls.push(poll);
      return poll.promise;
    },
    scheduleStatusPolls: (tick) => {
      const entry = { tick, owner: streams.length - 1, cancelled: false };
      ticks.push(entry);
      return () => {
        entry.cancelled = true;
      };
    },
    onPhase: (phase) => phases.push(phase),
    onFailure: (category) => failures.push(category),
  });
  return {
    streams,
    sends,
    polls,
    ticks,
    phases,
    failures,
    streamClosed,
    controller,
    runTick: (entry: (typeof ticks)[number]) => {
      pollOwner = entry.owner;
      entry.tick();
    },
  };
}

type Real = ReturnType<typeof harness>;

function streamCommand(
  model: ObservationModel,
  real: Real,
  command: Command
): void {
  const handlers = latest(real.streams, command.target);
  if (!handlers) return;
  const owner = real.streams.indexOf(handlers);
  if (command.kind === "ack") {
    model.observe(owner, "ack");
    handlers.onEvent({
      type: "tool_completed",
      name: "restart",
      success: true,
    });
  } else if (command.kind === "error") {
    model.observe(owner, "down");
    handlers.onError();
  } else {
    model.observe(owner, "up");
    handlers.onOpen();
  }
}

function settleSend(
  model: ObservationModel,
  real: Real,
  command: Command
): void {
  const send = latest(real.sends, command.target);
  if (!send || send.settled) return;
  send.settled = true;
  if (command.kind === "send-failure") {
    model.fail(send.owner);
    send.resolve({ kind: "failed", category: "transport" });
  } else {
    send.resolve({ kind: "accepted" });
  }
}

function settlePoll(
  model: ObservationModel,
  real: Real,
  command: Command
): void {
  const poll = latest(real.polls, command.target);
  if (!poll || poll.settled) return;
  poll.settled = true;
  model.observe(poll.owner, command.kind === "reachable" ? "up" : "down");
  if (command.kind === "poll-error") poll.reject(new Error("Unavailable"));
  else poll.resolve(command.kind === "reachable");
}

/** Issue the retained timer callback while counting only live model polls. */
function issuePoll(
  model: ObservationModel,
  real: Real,
  command: Command
): void {
  const entry = latest(real.ticks, command.target);
  if (!entry) return;
  if (!model.closed && model.active === entry.owner) model.pollsStarted += 1;
  real.runTick(entry);
}

// Exhaustive command dispatch keeps transport actions separate from the
// observation oracle, and requires an explicit handler for every new command.
const commandActions: Record<
  CommandKind,
  (model: ObservationModel, real: Real, command: Command) => void
> = {
  start: (model, real) => {
    model.start();
    real.controller.start();
  },
  ack: streamCommand,
  open: streamCommand,
  error: streamCommand,
  dispose: (model, real) => {
    model.dispose();
    real.controller.dispose();
  },
  "send-success": settleSend,
  "send-failure": settleSend,
  reachable: settlePoll,
  unreachable: settlePoll,
  "poll-error": settlePoll,
  poll: issuePoll,
};

const ordinaryCommand = fc.constantFrom<CommandKind>(
  "start",
  "ack",
  "open",
  "error",
  "poll",
  "reachable",
  "unreachable",
  "poll-error",
  "send-success",
  "send-failure"
);
const disposeCommand = fc.constant<CommandKind>("dispose");

const commands = fc.array(
  fc.record({
    kind: fc.oneof(
      {
        weight: 30,
        arbitrary: ordinaryCommand,
      },
      { weight: 1, arbitrary: disposeCommand }
    ),
    target: fc.integer({ min: 0, max: 7 }),
  }),
  { maxLength: 100 }
);

describe("restart command histories", () => {
  it("matches acknowledgement/outage/recovery witnesses and isolates retired attempts", async () => {
    await fc.assert(
      fc.asyncProperty(commands, async (history) => {
        const real = harness();
        const model = new ObservationModel();
        try {
          const initialCommand: Command = { kind: "start", target: 0 };
          for (const command of [initialCommand, ...history]) {
            commandActions[command.kind](model, real, command);
            await Promise.resolve();
            await Promise.resolve();
            expect(real.phases).toEqual(model.phases);
            expect(real.failures).toHaveLength(
              model.phases.filter((phase) => phase === "failed").length
            );
            expect(real.sends).toHaveLength(model.attempts.length);
            expect(real.streams).toHaveLength(model.attempts.length);
            expect(real.ticks).toHaveLength(model.timersStarted);
            expect(real.polls).toHaveLength(model.pollsStarted);
          }
        } finally {
          real.controller.dispose();
          expect(real.streamClosed.every(Boolean)).toBe(true);
          expect(real.ticks.every((entry) => entry.cancelled)).toBe(true);
        }
      }),
      { numRuns: 200 }
    );
  });

  it("ignores a failed attempt's stream and pending poll after retry", async () => {
    const real = harness();
    const model = new ObservationModel();
    const history: Command[] = [
      { kind: "start", target: 0 },
      { kind: "ack", target: 0 },
      { kind: "poll", target: 0 },
      { kind: "send-failure", target: 0 },
      { kind: "start", target: 0 },
      { kind: "ack", target: 0 },
      { kind: "unreachable", target: 0 },
      { kind: "open", target: 0 },
      { kind: "error", target: 1 },
      { kind: "open", target: 1 },
      { kind: "dispose", target: 0 },
      { kind: "ack", target: 0 },
    ];
    for (const command of history) {
      commandActions[command.kind](model, real, command);
      await Promise.resolve();
      await Promise.resolve();
      expect(real.phases).toEqual(model.phases);
    }
    expect(real.phases).toEqual(["restarting", "failed", "restarting"]);
    expect(real.ticks).toHaveLength(2);
    expect(real.streamClosed.every(Boolean)).toBe(true);
  });

  it("ignores a pending poll and all stream callbacks after disposal", async () => {
    const real = harness();
    const model = new ObservationModel();
    const history: Command[] = [
      { kind: "start", target: 0 },
      { kind: "ack", target: 0 },
      { kind: "poll", target: 0 },
      { kind: "error", target: 0 },
      { kind: "dispose", target: 0 },
      { kind: "reachable", target: 0 },
      { kind: "send-failure", target: 0 },
      { kind: "ack", target: 0 },
      { kind: "open", target: 0 },
      { kind: "poll", target: 0 },
      { kind: "start", target: 0 },
    ];
    for (const command of history) {
      commandActions[command.kind](model, real, command);
      await Promise.resolve();
      await Promise.resolve();
      expect(real.phases).toEqual(model.phases);
    }
    expect(real.phases).toEqual(["restarting"]);
    expect(real.streams).toHaveLength(1);
    expect(real.ticks).toHaveLength(1);
    expect(real.polls).toHaveLength(1);
    expect(real.streamClosed.every(Boolean)).toBe(true);
    expect(real.ticks.every((entry) => entry.cancelled)).toBe(true);
  });
});
