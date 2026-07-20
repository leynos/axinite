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
import { isStreamingApiPath } from "./streaming-routes";
import { MockBackendState, PairingRateLimitedError } from "./state";

export const DEFAULT_API_PORT = Number(process.env.MOCK_API_PORT ?? "8787");

// Deterministic failure fixtures: MOCK_FAILURES is a comma-separated list of
// request paths that should return HTTP 500 so error-handling UI states can
// be exercised without the real daemon (for example
// MOCK_FAILURES=/api/jobs,/api/skills).
export function parseFailureRoutes(raw: string | undefined): Set<string> {
  return new Set(
    (raw ?? "")
      .split(",")
      .map((entry) => entry.trim())
      .filter((entry) => entry.length > 0)
  );
}

const failureRoutes = parseFailureRoutes(process.env.MOCK_FAILURES);

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

function buildChatSseResponse(state: MockBackendState): Response {
  const encoder = new TextEncoder();
  let cleanup: (() => void) | undefined;
  let heartbeat: ReturnType<typeof setInterval> | undefined;

  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      const send = (event: ChatSseEvent) => {
        controller.enqueue(
          encoder.encode(
            `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`
          )
        );
      };

      cleanup = state.subscribeToChat({
        send,
        close: () => controller.close(),
      });

      heartbeat = setInterval(() => {
        send({ type: "heartbeat" });
      }, 15_000);
    },
    cancel() {
      cleanup?.();
      if (heartbeat) {
        clearInterval(heartbeat);
      }
    },
  });

  return new Response(stream, {
    headers: {
      "content-type": "text/event-stream",
      "cache-control": "no-cache",
      connection: "keep-alive",
      "x-accel-buffering": "no",
    },
  });
}

function buildLogSseResponse(state: MockBackendState): Response {
  const encoder = new TextEncoder();
  let cleanup: (() => void) | undefined;
  let heartbeat: ReturnType<typeof setInterval> | undefined;

  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      const send = (entry: LogEntry) => {
        controller.enqueue(
          encoder.encode(`event: log\ndata: ${JSON.stringify(entry)}\n\n`)
        );
      };

      cleanup = state.subscribeToLogs({
        send,
        close: () => controller.close(),
      });

      heartbeat = setInterval(() => {
        controller.enqueue(encoder.encode(": keep-alive\n\n"));
      }, 15_000);
    },
    cancel() {
      cleanup?.();
      if (heartbeat) {
        clearInterval(heartbeat);
      }
    },
  });

  return new Response(stream, {
    headers: {
      "content-type": "text/event-stream",
      "cache-control": "no-cache",
      connection: "keep-alive",
      "x-accel-buffering": "no",
    },
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
        return jsonResponse(state.pairingApprove(match![1], body.code));
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
    handler: ({ state, match }) => jsonResponse(state.pairingList(match![1])),
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
    handler: ({ state, match }) => jsonResponse(state.getJob(match![1])),
  },
  {
    method: "GET",
    pattern: /^\/api\/jobs\/([^/]+)\/events$/,
    handler: ({ state, match }) => jsonResponse(state.getJobEvents(match![1])),
  },
  {
    method: "GET",
    pattern: /^\/api\/jobs\/([^/]+)\/files\/list$/,
    handler: ({ state, match }) => jsonResponse(state.listJobFiles(match![1])),
  },
  {
    method: "GET",
    pattern: /^\/api\/jobs\/([^/]+)\/files\/read$/,
    handler: ({ state, url, match }) => {
      const path = url.searchParams.get("path");
      if (!path) {
        return errorResponse(400, "Missing path query parameter.");
      }
      return jsonResponse(state.readJobFile(match![1], path));
    },
  },
  {
    method: "POST",
    pattern: /^\/api\/jobs\/([^/]+)\/restart$/,
    handler: ({ state, match }) => jsonResponse(state.restartJob(match![1])),
  },
  {
    method: "POST",
    pattern: /^\/api\/jobs\/([^/]+)\/cancel$/,
    handler: ({ state, match }) => jsonResponse(state.cancelJob(match![1])),
  },
  {
    method: "POST",
    pattern: /^\/api\/jobs\/([^/]+)\/prompt$/,
    handler: async ({ state, request, match }) => {
      const body = await parseJson<JobPromptRequest>(request);
      return jsonResponse(state.promptJob(match![1], body));
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
    handler: ({ state, match }) => jsonResponse(state.getRoutine(match![1])),
  },
  {
    method: "GET",
    pattern: /^\/api\/routines\/([^/]+)\/runs$/,
    handler: ({ state, match }) =>
      jsonResponse(state.getRoutineRuns(match![1])),
  },
  {
    method: "POST",
    pattern: /^\/api\/routines\/([^/]+)\/trigger$/,
    handler: ({ state, match }) =>
      jsonResponse(state.triggerRoutine(match![1])),
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
      return jsonResponse(state.toggleRoutine(match![1], body));
    },
  },
  {
    method: "DELETE",
    pattern: /^\/api\/routines\/([^/]+)$/,
    handler: ({ state, match }) => jsonResponse(state.deleteRoutine(match![1])),
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
      jsonResponse(state.searchExtensionRegistry(url.searchParams.get("query"))),
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
      jsonResponse(state.activateExtension(match![1])),
  },
  {
    method: "POST",
    pattern: /^\/api\/extensions\/([^/]+)\/remove$/,
    handler: ({ state, match }) =>
      jsonResponse(state.removeExtension(match![1])),
  },
  {
    method: "GET",
    pattern: /^\/api\/extensions\/([^/]+)\/setup$/,
    handler: ({ state, match }) =>
      jsonResponse(state.getExtensionSetup(match![1])),
  },
  {
    method: "POST",
    pattern: /^\/api\/extensions\/([^/]+)\/setup$/,
    handler: async ({ state, request, match }) => {
      const body = await parseJson<ExtensionSetupRequest>(request);
      return jsonResponse(state.submitExtensionSetup(match![1], body.secrets));
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
    handler: ({ state, match }) => jsonResponse(state.removeSkill(match![1])),
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

// Attempt to match a single route. Returns the captured groups (null for an
// exact-string match) when both method and path match, otherwise undefined.
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

export async function handleMockRequest(
  request: Request,
  state: MockBackendState,
  failures: Set<string> = failureRoutes
): Promise<Response> {
  const url = new URL(request.url);
  const pathname = url.pathname;
  const method = request.method.toUpperCase();

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
    return errorResponse(
      400,
      error instanceof Error ? error.message : "Unknown mock backend error."
    );
  }

  return errorResponse(404, `No mock route for ${method} ${pathname}.`);
}

export function createMockBackendServer(
  port = DEFAULT_API_PORT,
  state = new MockBackendState()
): {
  port: number;
  server: ReturnType<typeof Bun.serve>;
  state: MockBackendState;
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
  return { port: server.port ?? port, server, state };
}

if (import.meta.main) {
  const { port } = createMockBackendServer();
  console.log(`[mock-api] listening on http://localhost:${port}`);
}
