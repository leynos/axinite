/**
 * @file Mock gateway HTTP server. It answers the `/api/*` routes the SPA calls,
 * including the chat and log SSE streams, from in-memory `MockBackendState`
 * so the frontend can be developed and tested without the Rust daemon.
 * `MOCK_FAILURES` forces HTTP 500 on chosen paths; `MOCK_RESTART_FIXTURE=1`
 * enables synthetic downtime without restarting any process.
 */

import type {
  ApprovalRequest,
  AuthCancelRequest,
  AuthTokenRequest,
  ChatSseEvent,
  ExtensionSetupRequest,
  JobPromptRequest,
  LogEntry,
  MemorySearchRequest,
  MemoryWriteRequest,
  PairingApproveRequest,
  SendMessageRequest,
  SkillInstallRequest,
  SkillSearchRequest,
  ToggleRequest,
} from "../../axinite/src/lib/api/contracts";
import { parseMockFeatureFlagEnvironment } from "./flag-environment";
import { parsePort } from "./ports";
import { MockBackendState, PairingRateLimitedError } from "./state";
import { isStreamingApiPath } from "./streaming-routes";

export const DEFAULT_API_PORT = parsePort(
  "MOCK_API_PORT",
  process.env["MOCK_API_PORT"],
  8787
);

// Deterministic failure fixtures: MOCK_FAILURES is a comma-separated list of
// request paths that should return HTTP 500 so error-handling UI states can
// be exercised without the real daemon (for example
// MOCK_FAILURES=/api/jobs,/api/skills).
/**
 * Parses comma-separated paths for deterministic HTTP 500 fixtures; blank
 * entries are discarded.
 */
export function parseFailureRoutes(raw: string | undefined): Set<string> {
  return new Set(
    (raw ?? "")
      .split(",")
      .map((entry) => entry.trim())
      .filter((entry) => entry.length > 0)
  );
}

const failureRoutes = parseFailureRoutes(process.env["MOCK_FAILURES"]);

function jsonResponse(payload: unknown, init?: ResponseInit): Response {
  return new Response(JSON.stringify(payload), {
    headers: {
      "content-type": "application/json; charset=utf-8",
      "cache-control": "no-cache",
    },
    ...init,
  });
}

function errorResponse(status: number, message: string): Response {
  return jsonResponse({ error: message }, { status });
}

async function parseJson<T>(request: Request): Promise<T> {
  return (await request.json()) as T;
}

const SSE_HEADERS = {
  "content-type": "text/event-stream",
  "cache-control": "no-cache",
  connection: "keep-alive",
  "x-accel-buffering": "no",
};

type SseStreamSetup = {
  /** Register the subscriber; returns its unsubscribe function. */
  subscribe: (subscriber: {
    send: (frame: string) => void;
    close: () => void;
  }) => () => void;
  /** Frame written every 15 s to keep proxies from idling the stream out. */
  heartbeatFrame: string;
};

/**
 * Creates an SSE response with a 15-second heartbeat. State closure and
 * client cancellation share exactly-once teardown, and frames after
 * teardown are dropped to avoid enqueueing on a closed controller.
 */
export function buildSseResponse({
  subscribe,
  heartbeatFrame,
}: SseStreamSetup): Response {
  const encoder = new TextEncoder();
  let finished = false;
  let unsubscribe: (() => void) | undefined;
  let heartbeat: ReturnType<typeof setInterval> | undefined;
  let streamController: ReadableStreamDefaultController<Uint8Array>;

  const cleanup = () => {
    if (finished) {
      return;
    }
    finished = true;
    unsubscribe?.();
    if (heartbeat) {
      clearInterval(heartbeat);
    }
    try {
      streamController.close();
    } catch {
      // Already closed or cancelled by the client.
    }
  };

  const write = (frame: string) => {
    if (!finished) {
      streamController.enqueue(encoder.encode(frame));
    }
  };

  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      streamController = controller;
      unsubscribe = subscribe({ send: write, close: cleanup });
      heartbeat = setInterval(() => write(heartbeatFrame), 15_000);
    },
    cancel() {
      cleanup();
    },
  });

  return new Response(stream, { headers: SSE_HEADERS });
}

function buildChatSseResponse(state: MockBackendState): Response {
  const frame = (event: ChatSseEvent) =>
    `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`;
  return buildSseResponse({
    subscribe: ({ send, close }) =>
      state.subscribeToChat({
        send: (event) => send(frame(event)),
        close,
      }),
    heartbeatFrame: frame({ type: "heartbeat" }),
  });
}

function buildLogSseResponse(state: MockBackendState): Response {
  return buildSseResponse({
    subscribe: ({ send, close }) =>
      state.subscribeToLogs({
        send: (entry: LogEntry) =>
          send(`event: log\ndata: ${JSON.stringify(entry)}\n\n`),
        close,
      }),
    heartbeatFrame: ": keep-alive\n\n",
  });
}

// Route dispatch context. `match` carries the captured groups for regex
// patterns and is null for exact string patterns.
interface RouteContext {
  state: MockBackendState;
  url: URL;
  request: Request;
  match: RegExpMatchArray | null;
}

// Regex routes always capture a first group, so a missing one means the
// route table itself is wrong rather than the request.
function firstCapture(match: RegExpMatchArray | null): string {
  const value = match?.[1];
  if (value === undefined) {
    throw new Error("Route pattern did not capture a path parameter");
  }
  return value;
}

type RouteHandler = (ctx: RouteContext) => Response | Promise<Response>;

interface Route {
  method: string | readonly string[];
  pattern: string | RegExp;
  handler: RouteHandler;
}

// Declarative route table iterated once by handleMockRequest. Order is
// significant: earlier entries win, so more specific exact paths (such as
// /api/routines/summary) must precede the parametric patterns that would
// otherwise capture them (such as /api/routines/:id).
const routes: readonly Route[] = [
  {
    method: "GET",
    pattern: "/api/gateway/status",
    handler: ({ state }) => jsonResponse(state.getGatewayStatus()),
  },
  {
    method: "GET",
    pattern: "/api/features",
    handler: ({ state }) => jsonResponse(state.getFeatureFlags()),
  },
  {
    method: "GET",
    pattern: "/api/chat/threads",
    handler: ({ state }) => jsonResponse(state.listThreads()),
  },
  {
    method: "POST",
    pattern: "/api/chat/thread/new",
    handler: ({ state }) => jsonResponse(state.createThread(), { status: 201 }),
  },
  {
    method: "GET",
    pattern: "/api/chat/history",
    handler: ({ state, url }) =>
      jsonResponse(state.getHistory(url.searchParams.get("thread_id"))),
  },
  {
    method: "POST",
    pattern: "/api/chat/send",
    handler: async ({ state, request }) => {
      const body = await parseJson<SendMessageRequest>(request);
      return jsonResponse(state.sendMessage(body), { status: 202 });
    },
  },
  {
    method: "GET",
    pattern: "/api/chat/events",
    handler: ({ state }) => buildChatSseResponse(state),
  },
  {
    method: "POST",
    pattern: "/api/chat/approval",
    handler: async ({ state, request }) => {
      const body = await parseJson<ApprovalRequest>(request);
      return jsonResponse(state.submitApproval(body));
    },
  },
  {
    method: "POST",
    pattern: "/api/chat/auth-token",
    handler: async ({ state, request }) => {
      const body = await parseJson<AuthTokenRequest>(request);
      return jsonResponse(state.chatAuthToken(body));
    },
  },
  {
    method: "POST",
    pattern: "/api/chat/auth-cancel",
    handler: async ({ state, request }) => {
      const body = await parseJson<AuthCancelRequest>(request);
      return jsonResponse(state.chatAuthCancel(body));
    },
  },
  {
    method: "POST",
    pattern: /^\/api\/pairing\/([^/]+)\/approve$/,
    handler: async ({ state, request, match }) => {
      const body = await parseJson<PairingApproveRequest>(request);
      try {
        return jsonResponse(
          state.pairingApprove(firstCapture(match), body.code)
        );
      } catch (error) {
        if (error instanceof PairingRateLimitedError) {
          return new Response(error.message, {
            status: 429,
            headers: { "content-type": "text/plain; charset=utf-8" },
          });
        }
        throw error;
      }
    },
  },
  {
    method: "GET",
    pattern: /^\/api\/pairing\/([^/]+)$/,
    handler: ({ state, match }) =>
      jsonResponse(state.pairingList(firstCapture(match))),
  },
  {
    method: "GET",
    pattern: "/api/memory/tree",
    handler: ({ state, url }) => {
      const depthParam = url.searchParams.get("depth");
      return jsonResponse(
        state.getMemoryTree(
          depthParam === null ? undefined : Number.parseInt(depthParam, 10)
        )
      );
    },
  },
  {
    method: "GET",
    pattern: "/api/memory/list",
    handler: ({ state, url }) =>
      jsonResponse(state.listMemory(url.searchParams.get("path") ?? "")),
  },
  {
    method: "GET",
    pattern: "/api/memory/read",
    handler: ({ state, url }) => {
      const path = url.searchParams.get("path");
      if (!path) {
        return errorResponse(400, "Missing path query parameter.");
      }
      return jsonResponse(state.readMemory(path));
    },
  },
  {
    method: "POST",
    pattern: "/api/memory/write",
    handler: async ({ state, request }) => {
      const body = await parseJson<MemoryWriteRequest>(request);
      return jsonResponse(state.writeMemory(body));
    },
  },
  {
    method: "POST",
    pattern: "/api/memory/search",
    handler: async ({ state, request }) => {
      const body = await parseJson<MemorySearchRequest>(request);
      return jsonResponse(state.searchMemory(body));
    },
  },
  {
    method: "GET",
    pattern: "/api/jobs",
    handler: ({ state }) => jsonResponse(state.listJobs()),
  },
  {
    method: "GET",
    pattern: "/api/jobs/summary",
    handler: ({ state }) => jsonResponse(state.summarizeJobs()),
  },
  {
    method: "GET",
    pattern: /^\/api\/jobs\/([^/]+)$/,
    handler: ({ state, match }) =>
      jsonResponse(state.getJob(firstCapture(match))),
  },
  {
    method: "GET",
    pattern: /^\/api\/jobs\/([^/]+)\/events$/,
    handler: ({ state, match }) =>
      jsonResponse(state.getJobEvents(firstCapture(match))),
  },
  {
    method: "GET",
    pattern: /^\/api\/jobs\/([^/]+)\/files\/list$/,
    handler: ({ state, match }) =>
      jsonResponse(state.listJobFiles(firstCapture(match))),
  },
  {
    method: "GET",
    pattern: /^\/api\/jobs\/([^/]+)\/files\/read$/,
    handler: ({ state, url, match }) => {
      const path = url.searchParams.get("path");
      if (!path) {
        return errorResponse(400, "Missing path query parameter.");
      }
      return jsonResponse(state.readJobFile(firstCapture(match), path));
    },
  },
  {
    method: "POST",
    pattern: /^\/api\/jobs\/([^/]+)\/restart$/,
    handler: ({ state, match }) =>
      jsonResponse(state.restartJob(firstCapture(match))),
  },
  {
    method: "POST",
    pattern: /^\/api\/jobs\/([^/]+)\/cancel$/,
    handler: ({ state, match }) =>
      jsonResponse(state.cancelJob(firstCapture(match))),
  },
  {
    method: "POST",
    pattern: /^\/api\/jobs\/([^/]+)\/prompt$/,
    handler: async ({ state, request, match }) => {
      const body = await parseJson<JobPromptRequest>(request);
      return jsonResponse(state.promptJob(firstCapture(match), body));
    },
  },
  {
    method: "GET",
    pattern: "/api/routines",
    handler: ({ state }) => jsonResponse(state.listRoutines()),
  },
  {
    method: "GET",
    pattern: "/api/routines/summary",
    handler: ({ state }) => jsonResponse(state.summarizeRoutines()),
  },
  {
    method: "GET",
    pattern: /^\/api\/routines\/([^/]+)$/,
    handler: ({ state, match }) =>
      jsonResponse(state.getRoutine(firstCapture(match))),
  },
  {
    method: "GET",
    pattern: /^\/api\/routines\/([^/]+)\/runs$/,
    handler: ({ state, match }) =>
      jsonResponse(state.getRoutineRuns(firstCapture(match))),
  },
  {
    method: "POST",
    pattern: /^\/api\/routines\/([^/]+)\/trigger$/,
    handler: ({ state, match }) =>
      jsonResponse(state.triggerRoutine(firstCapture(match))),
  },
  {
    method: "POST",
    pattern: /^\/api\/routines\/([^/]+)\/toggle$/,
    handler: async ({ state, request, match }) => {
      const body =
        request.headers.get("content-length") === "0"
          ? undefined
          : await request
              .clone()
              .json()
              .catch(() => undefined as ToggleRequest | undefined);
      return jsonResponse(state.toggleRoutine(firstCapture(match), body));
    },
  },
  {
    method: "DELETE",
    pattern: /^\/api\/routines\/([^/]+)$/,
    handler: ({ state, match }) =>
      jsonResponse(state.deleteRoutine(firstCapture(match))),
  },
  {
    method: "GET",
    pattern: "/api/extensions",
    handler: ({ state }) => jsonResponse(state.listExtensions()),
  },
  {
    method: "GET",
    pattern: "/api/extensions/tools",
    handler: ({ state }) => jsonResponse(state.listExtensionTools()),
  },
  {
    method: "GET",
    pattern: "/api/extensions/registry",
    handler: ({ state, url }) =>
      jsonResponse(
        state.searchExtensionRegistry(url.searchParams.get("query"))
      ),
  },
  {
    method: "POST",
    pattern: "/api/extensions/install",
    handler: async ({ state, request }) => {
      const body = (await request.json().catch(() => ({ name: "" }))) as {
        name?: string;
      };
      if (!body.name) {
        return errorResponse(400, "Missing extension name.");
      }
      return jsonResponse(state.installExtension(body.name));
    },
  },
  {
    method: "POST",
    pattern: /^\/api\/extensions\/([^/]+)\/activate$/,
    handler: ({ state, match }) =>
      jsonResponse(state.activateExtension(firstCapture(match))),
  },
  {
    method: "POST",
    pattern: /^\/api\/extensions\/([^/]+)\/remove$/,
    handler: ({ state, match }) =>
      jsonResponse(state.removeExtension(firstCapture(match))),
  },
  {
    method: "GET",
    pattern: /^\/api\/extensions\/([^/]+)\/setup$/,
    handler: ({ state, match }) =>
      jsonResponse(state.getExtensionSetup(firstCapture(match))),
  },
  {
    method: "POST",
    pattern: /^\/api\/extensions\/([^/]+)\/setup$/,
    handler: async ({ state, request, match }) => {
      const body = await parseJson<ExtensionSetupRequest>(request);
      return jsonResponse(
        state.submitExtensionSetup(firstCapture(match), body.secrets)
      );
    },
  },
  {
    method: "GET",
    pattern: "/api/skills",
    handler: ({ state }) => jsonResponse(state.listSkills()),
  },
  {
    method: "POST",
    pattern: "/api/skills/search",
    handler: async ({ state, request }) => {
      const body = await parseJson<SkillSearchRequest>(request);
      return jsonResponse(state.searchSkills(body));
    },
  },
  {
    method: "POST",
    pattern: "/api/skills/install",
    handler: async ({ state, request }) => {
      const body = await parseJson<SkillInstallRequest>(request);
      return jsonResponse(state.installSkill(body));
    },
  },
  {
    method: "DELETE",
    pattern: /^\/api\/skills\/([^/]+)$/,
    handler: ({ state, match }) =>
      jsonResponse(state.removeSkill(firstCapture(match))),
  },
  {
    method: "GET",
    pattern: "/api/logs/events",
    handler: ({ state }) => buildLogSseResponse(state),
  },
  {
    method: "GET",
    pattern: "/api/logs/level",
    handler: ({ state }) => jsonResponse(state.getLogLevel()),
  },
  {
    method: ["POST", "PUT"],
    pattern: "/api/logs/level",
    handler: async ({ state, request }) => {
      const body = (await request.json().catch(() => ({ level: "info" }))) as {
        level?: string;
      };
      return jsonResponse(state.setLogLevel(body.level ?? "info"));
    },
  },
];

/**
 * Match a route's allowed method and pathname. Exact-string matches return
 * `null`; regex matches return the `RegExpMatchArray` (full match at index 0,
 * capture groups from index 1). `undefined` means either method or path did
 * not match, so dispatch can distinguish a successful exact match from no
 * route. Parameter handlers use `firstCapture` for the captured path value.
 */
function matchRoute(
  route: Route,
  method: string,
  pathname: string
): RegExpMatchArray | null | undefined {
  const methods =
    typeof route.method === "string" ? [route.method] : route.method;
  if (!methods.includes(method)) {
    return undefined;
  }
  if (typeof route.pattern === "string") {
    return route.pattern === pathname ? null : undefined;
  }
  return pathname.match(route.pattern) ?? undefined;
}

/**
 * Dispatches by uppercased method and URL path. Injected failure paths
 * return 500, handler errors are logged and returned as 400, and unmatched
 * routes return 404.
 */
export async function handleMockRequest(
  request: Request,
  state: MockBackendState,
  failures: Set<string> = failureRoutes
): Promise<Response> {
  const url = new URL(request.url);
  const pathname = url.pathname;
  const method = request.method.toUpperCase();

  if (state.isGatewayUnavailable && pathname.startsWith("/api/")) {
    return errorResponse(503, "Simulated gateway restart in progress.");
  }

  if (failures.has(pathname)) {
    return errorResponse(500, `Simulated failure for ${pathname}.`);
  }

  try {
    for (const route of routes) {
      const match = matchRoute(route, method, pathname);
      if (match !== undefined) {
        return await route.handler({ state, url, request, match });
      }
    }
  } catch (error) {
    // Handlers throw to reject bad input (surfaced as 400), but a genuine
    // handler bug takes the same path, so record it server-side too.
    console.error(`[mock-api] ${method} ${pathname} failed:`, error);
    return errorResponse(
      400,
      error instanceof Error ? error.message : "Unknown mock backend error."
    );
  }

  return errorResponse(404, `No mock route for ${method} ${pathname}.`);
}

/**
 * Starts the Bun mock API on the requested port with the supplied state,
 * disables request timeouts for streaming routes and returns the bound
 * port, server, state and an owned stop hook. The default state enables
 * synthetic restart downtime only when MOCK_RESTART_FIXTURE=1.
 */
export function createMockBackendServer(
  port = DEFAULT_API_PORT,
  state = new MockBackendState(undefined, {
    restartFixture: process.env["MOCK_RESTART_FIXTURE"] === "1",
    featureFlagOverrides: parseMockFeatureFlagEnvironment(process.env),
  })
): {
  port: number;
  server: ReturnType<typeof Bun.serve>;
  state: MockBackendState;
  /** Stops this owned server and disposes its restart fixture timers. */
  stop: () => void;
} {
  const server = Bun.serve({
    port,
    fetch(request, server) {
      const pathname = new URL(request.url).pathname;
      if (isStreamingApiPath(pathname)) {
        server.timeout(request, 0);
      }
      return handleMockRequest(request, state);
    },
  });
  const stop = () => {
    state.dispose();
    server.stop(true);
  };
  return { port: server.port ?? port, server, state, stop };
}

if (import.meta.main) {
  const { port, stop } = createMockBackendServer();
  process.once("SIGINT", () => {
    stop();
    process.exit(0);
  });
  process.once("SIGTERM", () => {
    stop();
    process.exit(0);
  });
  console.log(`[mock-api] listening on http://localhost:${port}`);
}
