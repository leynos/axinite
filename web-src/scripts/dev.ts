/**
 * @file Local development runtime: choose a preview port, launch the mock API,
 * Vite watcher and preview, and stop only those owned children on exit/signals.
 * Imports expose injected helpers without starting processes or probing ports.
 */
import net from "node:net";
import { parsePort } from "../mock-backend/src/ports";

/** Minimal disposable TCP probe capability; it does not reserve a port. */
export type PortProbe = {
  unref: () => void;
  once: (event: "error" | "listening", listener: () => void) => void;
  listen: (port: number, host: string) => void;
  close: (callback: () => void) => void;
};

/**
 * Probes localhost and closes the probe before resolving. Bind errors resolve
 * false; a successful bind resolves true after close, without reserving a port.
 */
export async function isPortAvailable(
  port: number,
  createProbe: () => PortProbe = () => net.createServer()
): Promise<boolean> {
  return await new Promise((resolve) => {
    const server = createProbe();
    server.unref();
    server.once("error", () => server.close(() => resolve(false)));
    server.once("listening", () => server.close(() => resolve(true)));
    server.listen(port, "127.0.0.1");
  });
}

/**
 * Selects the default port or the next available port within the following 100
 * TCP ports, clamped to 65535. An explicitly configured occupied port rejects.
 * Probes close before returning, so selection is advisory rather than a reservation.
 */
export async function reservePreviewPort(options: {
  defaultPort: number;
  wasExplicit: boolean;
  isAvailable?: (port: number) => Promise<boolean>;
  warn?: (message: string) => void;
}): Promise<number> {
  const probe = options.isAvailable ?? isPortAvailable;
  if (await probe(options.defaultPort)) return options.defaultPort;
  if (options.wasExplicit) {
    throw new Error(
      `[dev] PREVIEW_PORT ${options.defaultPort} is unavailable. Choose a free port and retry.`
    );
  }
  const maxFallbackPort = Math.min(65_535, options.defaultPort + 100);
  for (
    let candidate = options.defaultPort + 1;
    candidate <= maxFallbackPort;
    candidate += 1
  ) {
    if (await probe(candidate)) {
      (options.warn ?? console.warn)(
        `[dev] preview port ${options.defaultPort} is busy; falling back to ${candidate}`
      );
      return candidate;
    }
  }
  throw new Error(
    `[dev] could not find a free preview port in ${options.defaultPort}-${maxFallbackPort}`
  );
}

/** The supervisor may signal only explicitly supplied owned subprocesses. */
export type ManagedProcess = {
  label: string;
  process: { kill: () => void; exited: Promise<number> };
};

/** Process lifecycle adapter; registering a signal returns its removal hook. */
export type SupervisorDependencies = {
  onSignal: (signal: "SIGINT" | "SIGTERM", handler: () => void) => () => void;
  log: (message: string) => void;
  error: (message: string) => void;
  exit: (code: number) => void;
};

/**
 * Waits for a child exit or operator signal, kills owned children once, waits
 * for all exits and removes signal listeners. Signals exit cleanly; an
 * unexpected child exit propagates its nonzero code, or 1 for an unexpected 0.
 */
export async function superviseChildren(
  managed: ManagedProcess[],
  deps: SupervisorDependencies
): Promise<void> {
  let shutdown: Promise<void> | undefined;
  const stopAll = (reason: string, exitCode: number): Promise<void> => {
    shutdown ??= Promise.resolve().then(async () => {
      deps.log(`[dev] ${reason}, shutting down child processes`);
      for (const child of managed) child.process.kill();
      await Promise.allSettled(managed.map((child) => child.process.exited));
      deps.exit(exitCode);
    });
    return shutdown;
  };
  const removeSignals = [
    deps.onSignal("SIGINT", () => {
      void stopAll("received SIGINT", 0);
    }),
    deps.onSignal("SIGTERM", () => {
      void stopAll("received SIGTERM", 0);
    }),
  ];
  try {
    const result = await Promise.race(
      managed.map(async (child) => ({
        label: child.label,
        exitCode: await child.process.exited.catch(() => 1),
      }))
    );
    if (shutdown) {
      await shutdown;
    } else {
      deps.error(
        `[dev] ${result.label} exited unexpectedly with code ${result.exitCode}`
      );
      await stopAll(`${result.label} exited`, result.exitCode || 1);
    }
  } finally {
    for (const remove of removeSignals) remove();
  }
}

function pipeOutput(
  label: string,
  stream: ReadableStream<Uint8Array> | null,
  writer: (line: string) => void
): void {
  if (!stream) return;
  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let pending = "";
  void (async () => {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) {
        if (pending.length > 0) writer(`[${label}] ${pending}`);
        break;
      }
      pending += decoder.decode(value, { stream: true });
      const lines = pending.split(/\r?\n/);
      pending = lines.pop() ?? "";
      for (const line of lines) writer(`[${label}] ${line}`);
    }
  })();
}

function spawnManaged(
  label: string,
  cmd: string[],
  env: NodeJS.ProcessEnv = process.env
): ManagedProcess {
  const child = Bun.spawn(cmd, {
    cwd: process.cwd(),
    env,
    stdout: "pipe",
    stderr: "pipe",
  });
  pipeOutput(label, child.stdout, console.log);
  pipeOutput(label, child.stderr, console.error);
  return { label, process: child };
}

async function main(): Promise<void> {
  const apiPort = parsePort(
    "MOCK_API_PORT",
    process.env["MOCK_API_PORT"],
    8787
  );
  const defaultPort = parsePort(
    "PREVIEW_PORT",
    process.env["PREVIEW_PORT"],
    2020
  );
  const previewPort = await reservePreviewPort({
    defaultPort,
    wasExplicit: process.env["PREVIEW_PORT"] !== undefined,
  });
  console.log(
    `[dev] starting mock API on ${apiPort}, static preview on ${previewPort}`
  );
  const managed = [
    spawnManaged("mock-api", ["bun", "--watch", "mock-backend/src/server.ts"], {
      ...process.env,
      MOCK_API_PORT: String(apiPort),
    }),
    spawnManaged("build", ["bunx", "vite", "build", "--watch"]),
    spawnManaged("preview", ["bun", "mock-backend/src/preview-server.ts"], {
      ...process.env,
      MOCK_API_PORT: String(apiPort),
      PREVIEW_PORT: String(previewPort),
    }),
  ];
  await superviseChildren(managed, {
    onSignal: (signal, handler) => {
      process.on(signal, handler);
      return () => {
        process.off(signal, handler);
      };
    },
    log: console.log,
    error: console.error,
    exit: (code) => {
      process.exitCode = code;
    },
  });
}

if (import.meta.main) await main();
