/** @file Preview startup follows the initial build and preserves child supervision. */
import { describe, expect, it, vi } from "vitest";
import { createBuildReadiness } from "../../scripts/build-output";
import { superviseChildren } from "../../scripts/dev";

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

const flush = async () => {
  for (let step = 0; step < 8; step += 1) await Promise.resolve();
};

describe("preview startup supervision", () => {
  it.each([
    "built in 2066ms.",
    "built in 1.64s.",
    "\u001B[32mbuilt in 2.5s.\u001B[39m",
  ])("recognizes Vite completion line %s", async (line) => {
    const build = createBuildReadiness();
    let isReady = false;
    void build.ready.then(() => {
      isReady = true;
    });
    build.observeLine("build started...");
    build.observeLine("built in 1.64s");
    build.observeLine("error: built in 1.64s.");
    build.observeLine("built in 5min.");
    await flush();
    expect(isReady).toBe(false);
    build.observeLine(line);
    await build.ready;
    expect(isReady).toBe(true);
  });

  it.each([
    [new Error("spawn denied"), "spawn denied"],
    ["spawn unavailable", "spawn unavailable"],
  ])(
    "reports the cause of a preview startup failure",
    async (cause, message) => {
      const buildExit = deferred<number>();
      const build = {
        label: "build",
        process: { exited: buildExit.promise, kill: vi.fn() },
      };
      const error = vi.fn();
      const exit = vi.fn();
      const run = superviseChildren(
        [build],
        { onSignal: () => () => {}, log: vi.fn(), error, exit },
        {
          ready: Promise.resolve(),
          start: () => {
            throw cause;
          },
        }
      );
      await flush();
      expect(error).toHaveBeenCalledWith(
        `[dev] preview failed to start: ${message}`
      );
      expect(error).toHaveBeenCalledWith(
        "[dev] preview exited unexpectedly with code 1"
      );
      expect(build.process.kill).toHaveBeenCalledTimes(1);
      expect(exit).not.toHaveBeenCalled();
      buildExit.resolve(0);
      await run;
      expect(exit).toHaveBeenCalledWith(1);
    }
  );

  it("starts and supervises preview only after the initial build is ready", async () => {
    const ready = createBuildReadiness();
    const buildExit = deferred<number>();
    const previewExit = deferred<number>();
    const build = {
      label: "build",
      process: { exited: buildExit.promise, kill: vi.fn() },
    };
    const preview = {
      label: "preview",
      process: { exited: previewExit.promise, kill: vi.fn() },
    };
    const start = vi.fn(() => preview);
    const exit = vi.fn();
    const run = superviseChildren(
      [build],
      {
        onSignal: () => () => {},
        log: vi.fn(),
        error: vi.fn(),
        exit,
      },
      { ready: ready.ready, start }
    );
    await flush();
    expect(start).not.toHaveBeenCalled();
    ready.observeLine("build started...");
    await flush();
    expect(start).not.toHaveBeenCalled();
    ready.observeLine("\u001B[32mbuilt in 2066ms.\u001B[39m");
    await flush();
    expect(start).toHaveBeenCalledTimes(1);
    previewExit.resolve(7);
    await flush();
    expect(build.process.kill).toHaveBeenCalledTimes(1);
    buildExit.resolve(0);
    await run;
    expect(exit).toHaveBeenCalledWith(7);
  });

  it("does not start preview if the build exits before readiness", async () => {
    const ready = deferred<void>();
    const buildExit = deferred<number>();
    const start = vi.fn();
    const exit = vi.fn();
    const run = superviseChildren(
      [
        {
          label: "build",
          process: { exited: buildExit.promise, kill: vi.fn() },
        },
      ],
      {
        onSignal: () => () => {},
        log: vi.fn(),
        error: vi.fn(),
        exit,
      },
      { ready: ready.promise, start }
    );
    buildExit.resolve(2);
    await run;
    ready.resolve();
    await flush();
    expect(start).not.toHaveBeenCalled();
    expect(exit).toHaveBeenCalledWith(2);
  });

  it("does not start preview after a shutdown signal", async () => {
    const ready = deferred<void>();
    const buildExit = deferred<number>();
    const handlers = new Map<string, () => void>();
    const start = vi.fn();
    const exit = vi.fn();
    const run = superviseChildren(
      [
        {
          label: "build",
          process: { exited: buildExit.promise, kill: vi.fn() },
        },
      ],
      {
        onSignal: (signal, handler) => {
          handlers.set(signal, handler);
          return () => handlers.delete(signal);
        },
        log: vi.fn(),
        error: vi.fn(),
        exit,
      },
      { ready: ready.promise, start }
    );
    handlers.get("SIGTERM")?.();
    ready.resolve();
    buildExit.resolve(0);
    await run;
    expect(start).not.toHaveBeenCalled();
    expect(exit).toHaveBeenCalledWith(0);
  });
});
