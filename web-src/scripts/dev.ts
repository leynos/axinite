/**
 * Local development runner. It starts the mock API server, a watching Vite
 * build, and the static preview server together, prefixes their output,
 * picks a free preview port, and stops all three on a signal or if one exits.
 */

import net from "node:net";

import { parsePort } from "../mock-backend/src/ports";

const apiPort = parsePort("MOCK_API_PORT", process.env.MOCK_API_PORT, 8787);
const defaultPreviewPort = parsePort(
  "PREVIEW_PORT",
  process.env.PREVIEW_PORT,
  2020
);
const previewPortWasExplicit = process.env.PREVIEW_PORT !== undefined;

type ManagedProcess = {
  label: string;
  process: Bun.Subprocess;
};

function pipeOutput(
  label: string,
  stream: ReadableStream<Uint8Array> | null,
  writer: (line: string) => void
): void {
  if (!stream) {
    return;
  }

  const reader = stream.getReader();
  const decoder = new TextDecoder();
  let pending = "";

  void (async () => {
    while (true) {
      const { done, value } = await reader.read();
      if (done) {
        if (pending.length > 0) {
          writer(`[${label}] ${pending}`);
        }
        break;
      }

      pending += decoder.decode(value, { stream: true });
      const lines = pending.split(/\r?\n/);
      pending = lines.pop() ?? "";
      for (const line of lines) {
        writer(`[${label}] ${line}`);
      }
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

async function isPortAvailable(port: number): Promise<boolean> {
  return await new Promise((resolve) => {
    const server = net.createServer();
    server.unref();

    server.once("error", () => {
      resolve(false);
    });

    server.once("listening", () => {
      server.close(() => resolve(true));
    });

    server.listen(port, "127.0.0.1");
  });
}

async function reservePreviewPort(): Promise<number> {
  if (await isPortAvailable(defaultPreviewPort)) {
    return defaultPreviewPort;
  }

  if (previewPortWasExplicit) {
    throw new Error(
      `[dev] PREVIEW_PORT ${defaultPreviewPort} is unavailable. Choose a free port and retry.`
    );
  }

  const maxFallbackPort = defaultPreviewPort + 100;
  for (
    let candidatePort = defaultPreviewPort + 1;
    candidatePort <= maxFallbackPort;
    candidatePort += 1
  ) {
    if (await isPortAvailable(candidatePort)) {
      console.warn(
        `[dev] preview port ${defaultPreviewPort} is busy; falling back to ${candidatePort}`
      );
      return candidatePort;
    }
  }

  throw new Error(
    `[dev] could not find a free preview port in ${defaultPreviewPort}-${maxFallbackPort}`
  );
}

async function main(): Promise<void> {
  const previewPort = await reservePreviewPort();
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

  let shuttingDown = false;

  // Operator signals are a clean stop (exit 0); a child dying on its own is a
  // failure the caller (make frontend-stub, CI) must see as non-zero.
  const stopAll = async (reason: string, exitCode: number) => {
    if (shuttingDown) {
      return;
    }
    shuttingDown = true;
    console.log(`[dev] ${reason}, shutting down child processes`);
    for (const child of managed) {
      child.process.kill();
    }
    await Promise.allSettled(managed.map((child) => child.process.exited));
    process.exit(exitCode);
  };

  process.on("SIGINT", () => {
    void stopAll("received SIGINT", 0);
  });
  process.on("SIGTERM", () => {
    void stopAll("received SIGTERM", 0);
  });

  const results = await Promise.race(
    managed.map(async (child) => ({
      label: child.label,
      exitCode: await child.process.exited,
    }))
  );

  if (!shuttingDown) {
    console.error(
      `[dev] ${results.label} exited unexpectedly with code ${results.exitCode}`
    );
    await stopAll(`${results.label} exited`, results.exitCode || 1);
  }
}

await main();
