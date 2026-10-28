/** @file Bounded lifecycle histories for delayed preview supervision. */
import fc from "fast-check";
import { expect, it, vi } from "vitest";
import { superviseChildren } from "../../scripts/dev";

type History =
  | "build-exit"
  | "signal"
  | "signal-after-ready"
  | "start-failure"
  | "preview-exit";

function deferred<T>() {
  let resolve: ((value: T) => void) | undefined;
  const promise = new Promise<T>((accept) => {
    resolve = accept;
  });
  return {
    promise,
    resolve: (value: T) => {
      if (!resolve) throw new Error("Promise resolver was not initialized");
      resolve(value);
    },
  };
}

function supervisionHarness(history: History) {
  const readiness = deferred<void>();
  const buildExit = deferred<number>();
  const previewExit = deferred<number>();
  const handlers = new Map<string, () => void>();
  const build = {
    label: "build",
    process: {
      exited: buildExit.promise,
      kill: vi.fn(() => buildExit.resolve(0)),
    },
  };
  const preview = {
    label: "preview",
    process: {
      exited: previewExit.promise,
      kill: vi.fn(() => previewExit.resolve(0)),
    },
  };
  const start = vi.fn(() => {
    if (history === "start-failure") throw new Error("spawn failed");
    return preview;
  });
  const exit = vi.fn();
  const error = vi.fn();
  const run = superviseChildren(
    [build],
    {
      onSignal: (signal, handler) => {
        handlers.set(signal, handler);
        return () => {
          handlers.delete(signal);
        };
      },
      log: vi.fn(),
      error,
      exit,
    },
    { ready: readiness.promise, start }
  );
  return {
    readiness,
    buildExit,
    previewExit,
    handlers,
    build,
    preview,
    start,
    exit,
    error,
    run,
  };
}

type Harness = ReturnType<typeof supervisionHarness>;

function signalRepeatedly(harness: Harness, count: number): void {
  for (let index = 0; index < count; index += 1)
    harness.handlers.get(index % 2 === 0 ? "SIGTERM" : "SIGINT")?.();
}

async function exerciseHistory(
  harness: Harness,
  history: History,
  code: number,
  signalCount: number
): Promise<void> {
  if (history === "build-exit") {
    harness.buildExit.resolve(code);
  } else if (history === "signal") {
    signalRepeatedly(harness, signalCount);
  } else {
    harness.readiness.resolve();
    await vi.waitFor(() => expect(harness.start).toHaveBeenCalledTimes(1), {
      timeout: 500,
    });
    if (history === "preview-exit") harness.previewExit.resolve(code);
    if (history === "signal-after-ready")
      signalRepeatedly(harness, signalCount);
  }
  await vi.waitFor(() => expect(harness.exit).toHaveBeenCalledTimes(1), {
    timeout: 500,
  });
  await harness.run;
  harness.readiness.resolve();
  await Promise.resolve();
}

const expectedOutcomes: Record<History, {
  starts: number;
  previewKills: number;
  exit: "child" | "failure" | "signal";
  reportsStartupFailure: boolean;
}> = {
  "build-exit": { starts: 0, previewKills: 0, exit: "child", reportsStartupFailure: false },
  signal: { starts: 0, previewKills: 0, exit: "signal", reportsStartupFailure: false },
  "signal-after-ready": { starts: 1, previewKills: 1, exit: "signal", reportsStartupFailure: false },
  "start-failure": { starts: 1, previewKills: 0, exit: "failure", reportsStartupFailure: true },
  "preview-exit": { starts: 1, previewKills: 1, exit: "child", reportsStartupFailure: false },
};

function assertHistory(harness: Harness, history: History, code: number): void {
  const expected = expectedOutcomes[history];
  expect(harness.start).toHaveBeenCalledTimes(expected.starts);
  expect(harness.build.process.kill).toHaveBeenCalledTimes(1);
  expect(harness.preview.process.kill).toHaveBeenCalledTimes(expected.previewKills);
  const exitCodes = { child: code || 1, failure: 1, signal: 0 };
  expect(harness.exit).toHaveBeenCalledWith(exitCodes[expected.exit]);
  if (expected.reportsStartupFailure)
    expect(harness.error).toHaveBeenCalledWith(
      "[dev] preview failed to start: spawn failed"
    );
  expect(harness.handlers.size).toBe(0);
}

it("preserves ownership and exit rules across delayed-child histories", async () => {
  const scenario = fc.constantFrom<History>(
    "build-exit",
    "signal",
    "signal-after-ready",
    "start-failure",
    "preview-exit"
  );
  await fc.assert(
    fc.asyncProperty(
      scenario,
      fc.integer({ min: 0, max: 9 }),
      fc.integer({ min: 1, max: 4 }),
      async (history, code, signalCount) => {
        const harness = supervisionHarness(history);
        await exerciseHistory(harness, history, code, signalCount);
        assertHistory(harness, history, code);
      }
    ),
    { numRuns: 60 }
  );
});
