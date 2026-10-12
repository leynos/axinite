/** @file Port selection and owned-child lifecycle with injected probes and signals. */
import { describe, expect, it, vi } from "vitest";
import {
  isPortAvailable,
  type PortProbe,
  reservePreviewPort,
  superviseChildren,
} from "../../scripts/dev";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

const flush = async () => {
  for (let step = 0; step < 8; step += 1) await Promise.resolve();
};

describe("nonreserved preview port selection", () => {
  it.each([
    "listening",
    "error",
  ] as const)("closes the probe before resolving after %s", async (event) => {
    const listeners = new Map<string, () => void>();
    let closed: (() => void) | undefined;
    const probe: PortProbe = {
      unref: vi.fn(),
      once: vi.fn((name: "error" | "listening", handler: () => void) => {
        listeners.set(name, handler);
      }),
      listen: vi.fn(),
      close: vi.fn((callback: () => void) => {
        closed = callback;
      }),
    };
    let settled = false;
    const result = isPortAvailable(2020, () => probe).then((value) => {
      settled = true;
      return value;
    });
    expect(probe.listen).toHaveBeenCalledWith(2020, "127.0.0.1");
    listeners.get(event)?.();
    await flush();
    expect(settled).toBe(false);
    expect(probe.close).toHaveBeenCalledTimes(1);
    closed?.();
    expect(await result).toBe(event === "listening");
  });

  it("uses an available default without warnings or extra probes", async () => {
    const isAvailable = vi.fn().mockResolvedValue(true);
    const warn = vi.fn();
    expect(
      await reservePreviewPort({
        defaultPort: 2020,
        wasExplicit: false,
        isAvailable,
        warn,
      })
    ).toBe(2020);
    expect(isAvailable.mock.calls).toEqual([[2020]]);
    expect(warn).not.toHaveBeenCalled();
  });

  it("chooses the first free fallback and reports the selected port", async () => {
    const isAvailable = vi.fn(async (port: number) => port === 2023);
    const warn = vi.fn();
    expect(
      await reservePreviewPort({
        defaultPort: 2020,
        wasExplicit: false,
        isAvailable,
        warn,
      })
    ).toBe(2023);
    expect(isAvailable.mock.calls).toEqual([[2020], [2021], [2022], [2023]]);
    expect(warn).toHaveBeenCalledWith(
      "[dev] preview port 2020 is busy; falling back to 2023"
    );
  });

  it("rejects an occupied explicit port without probing alternatives", async () => {
    const isAvailable = vi.fn().mockResolvedValue(false);
    await expect(
      reservePreviewPort({ defaultPort: 2020, wasExplicit: true, isAvailable })
    ).rejects.toThrow("PREVIEW_PORT 2020 is unavailable");
    expect(isAvailable.mock.calls).toEqual([[2020]]);
  });

  it.each([
    [2020, 2120, 101],
    [65_534, 65_535, 2],
  ] as const)("bounds exhaustion from %s at %s", async (defaultPort, lastPort, probes) => {
    const isAvailable = vi.fn().mockResolvedValue(false);
    await expect(
      reservePreviewPort({ defaultPort, wasExplicit: false, isAvailable })
    ).rejects.toThrow(`${defaultPort}-${lastPort}`);
    expect(isAvailable).toHaveBeenCalledTimes(probes);
    expect(isAvailable.mock.calls.at(-1)).toEqual([lastPort]);
  });
});

function supervisorHarness() {
  const listeners = new Map<string, () => void>();
  const children = ["mock-api", "build", "preview"].map((label) => {
    const exit = deferred<number>();
    return { label, exit, process: { exited: exit.promise, kill: vi.fn() } };
  });
  const outsider = { kill: vi.fn() };
  const exit = vi.fn();
  const error = vi.fn();
  const log = vi.fn();
  const run = superviseChildren(children, {
    onSignal: (signal, handler) => {
      listeners.set(signal, handler);
      return () => {
        listeners.delete(signal);
      };
    },
    exit,
    error,
    log,
  });
  return { children, listeners, outsider, exit, error, log, run };
}

describe("owned development child supervision", () => {
  it.each([
    0, 7,
  ])("propagates unexpected child exit%s after waiting for all owned exits", async (code) => {
    const h = supervisorHarness();
    h.children[0]?.exit.resolve(code);
    await flush();
    for (const child of h.children)
      expect(child.process.kill).toHaveBeenCalledTimes(1);
    expect(h.exit).not.toHaveBeenCalled();
    for (const child of h.children.slice(1)) child.exit.resolve(0);
    await h.run;
    expect(h.exit).toHaveBeenCalledWith(code || 1);
    expect(h.error).toHaveBeenCalledWith(
      `[dev] mock-api exited unexpectedly with code ${code}`
    );
    expect(h.listeners.size).toBe(0);
    expect(h.outsider.kill).not.toHaveBeenCalled();
  });

  it("cleans up owned children when an exit promise rejects", async () => {
    const h = supervisorHarness();
    h.children[0]?.exit.reject(new Error("fixture wait failure"));
    await flush();
    for (const child of h.children.slice(1)) child.exit.resolve(0);
    await h.run;
    for (const child of h.children)
      expect(child.process.kill).toHaveBeenCalledTimes(1);
    expect(h.exit).toHaveBeenCalledWith(1);
    expect(h.listeners.size).toBe(0);
    expect(h.outsider.kill).not.toHaveBeenCalled();
  });

  it.each([
    "SIGINT",
    "SIGTERM",
  ])("treats %s as clean shutdown and deduplicates repeated signals", async (signal) => {
    const h = supervisorHarness();
    h.listeners.get(signal)?.();
    h.listeners.get(signal)?.();
    h.listeners.get("SIGTERM")?.();
    await flush();
    for (const child of h.children)
      expect(child.process.kill).toHaveBeenCalledTimes(1);
    expect(h.exit).not.toHaveBeenCalled();
    for (const child of h.children) child.exit.resolve(0);
    await h.run;
    expect(h.exit).toHaveBeenCalledTimes(1);
    expect(h.exit).toHaveBeenCalledWith(0);
    expect(h.error).not.toHaveBeenCalled();
    expect(h.listeners.size).toBe(0);
    expect(h.outsider.kill).not.toHaveBeenCalled();
  });
});
