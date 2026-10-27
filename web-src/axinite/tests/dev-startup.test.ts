/** @file Preview startup follows the initial build and preserves child supervision. */
import { describe, expect, it, vi } from "vitest";
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
  it("starts and supervises preview only after the initial build is ready", async () => {
    const ready = deferred<void>();
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
      { ready: ready.promise, start }
    );
    await flush();
    expect(start).not.toHaveBeenCalled();
    ready.resolve();
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
