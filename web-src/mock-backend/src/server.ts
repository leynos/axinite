import type {
  ApprovalRequest,
  ChatSseEvent,
  ExtensionSetupRequest,
  JobPromptRequest,
  LogEntry,
  MemorySearchRequest,
  MemoryWriteRequest,
  SendMessageRequest,
  SkillInstallRequest,
  SkillSearchRequest,
  ToggleRequest,
} from "../../axinite/src/lib/api/contracts";
import { isStreamingApiPath } from "./streaming-routes";
import { MockBackendState } from "./state";

export const DEFAULT_API_PORT = Number(process.env.MOCK_API_PORT ?? "8787");

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

export async function handleMockRequest(
  request: Request,
  state: MockBackendState
): Promise<Response> {
  const url = new URL(request.url);
  const pathname = url.pathname;
  const method = request.method.toUpperCase();

  try {
    if (method === "GET" && pathname === "/api/gateway/status") {
      return jsonResponse(state.getGatewayStatus());
    }

    if (method === "GET" && pathname === "/api/features") {
      return jsonResponse(state.getFeatureFlags());
    }

    if (method === "GET" && pathname === "/api/chat/threads") {
      return jsonResponse(state.listThreads());
    }

    if (method === "POST" && pathname === "/api/chat/thread/new") {
      return jsonResponse(state.createThread(), { status: 201 });
    }

    if (method === "GET" && pathname === "/api/chat/history") {
      return jsonResponse(state.getHistory(url.searchParams.get("thread_id")));
    }

    if (method === "POST" && pathname === "/api/chat/send") {
      const body = await parseJson<SendMessageRequest>(request);
      return jsonResponse(state.sendMessage(body), { status: 202 });
    }

    if (method === "GET" && pathname === "/api/chat/events") {
      return buildChatSseResponse(state);
    }

    if (method === "POST" && pathname === "/api/chat/approval") {
      const body = await parseJson<ApprovalRequest>(request);
      return jsonResponse(state.submitApproval(body));
    }

    if (method === "GET" && pathname === "/api/memory/tree") {
      const depthParam = url.searchParams.get("depth");
      return jsonResponse(
        state.getMemoryTree(
          depthParam === null ? undefined : Number.parseInt(depthParam, 10)
        )
      );
    }

    if (method === "GET" && pathname === "/api/memory/list") {
      return jsonResponse(state.listMemory(url.searchParams.get("path") ?? ""));
    }

    if (method === "GET" && pathname === "/api/memory/read") {
      const path = url.searchParams.get("path");
      if (!path) {
        return errorResponse(400, "Missing path query parameter.");
      }
      return jsonResponse(state.readMemory(path));
    }

    if (method === "POST" && pathname === "/api/memory/write") {
      const body = await parseJson<MemoryWriteRequest>(request);
      return jsonResponse(state.writeMemory(body));
    }

    if (method === "POST" && pathname === "/api/memory/search") {
      const body = await parseJson<MemorySearchRequest>(request);
      return jsonResponse(state.searchMemory(body));
    }

    if (method === "GET" && pathname === "/api/jobs") {
      return jsonResponse(state.listJobs());
    }

    if (method === "GET" && pathname === "/api/jobs/summary") {
      return jsonResponse(state.summarizeJobs());
    }

    const jobDetailMatch = pathname.match(/^\/api\/jobs\/([^/]+)$/);
    if (method === "GET" && jobDetailMatch) {
      return jsonResponse(state.getJob(jobDetailMatch[1]));
    }

    const jobEventsMatch = pathname.match(/^\/api\/jobs\/([^/]+)\/events$/);
    if (method === "GET" && jobEventsMatch) {
      return jsonResponse(state.getJobEvents(jobEventsMatch[1]));
    }

    const jobFilesListMatch = pathname.match(/^\/api\/jobs\/([^/]+)\/files\/list$/);
    if (method === "GET" && jobFilesListMatch) {
      return jsonResponse(state.listJobFiles(jobFilesListMatch[1]));
    }

    const jobFilesReadMatch = pathname.match(/^\/api\/jobs\/([^/]+)\/files\/read$/);
    if (method === "GET" && jobFilesReadMatch) {
      const path = url.searchParams.get("path");
      if (!path) {
        return errorResponse(400, "Missing path query parameter.");
      }
      return jsonResponse(state.readJobFile(jobFilesReadMatch[1], path));
    }

    const jobRestartMatch = pathname.match(/^\/api\/jobs\/([^/]+)\/restart$/);
    if (method === "POST" && jobRestartMatch) {
      return jsonResponse(state.restartJob(jobRestartMatch[1]));
    }

    const jobCancelMatch = pathname.match(/^\/api\/jobs\/([^/]+)\/cancel$/);
    if (method === "POST" && jobCancelMatch) {
      return jsonResponse(state.cancelJob(jobCancelMatch[1]));
    }

    const jobPromptMatch = pathname.match(/^\/api\/jobs\/([^/]+)\/prompt$/);
    if (method === "POST" && jobPromptMatch) {
      const body = await parseJson<JobPromptRequest>(request);
      return jsonResponse(state.promptJob(jobPromptMatch[1], body));
    }

    if (method === "GET" && pathname === "/api/routines") {
      return jsonResponse(state.listRoutines());
    }

    if (method === "GET" && pathname === "/api/routines/summary") {
      return jsonResponse(state.summarizeRoutines());
    }

    const routineDetailMatch = pathname.match(/^\/api\/routines\/([^/]+)$/);
    if (method === "GET" && routineDetailMatch) {
      return jsonResponse(state.getRoutine(routineDetailMatch[1]));
    }

    const routineRunsMatch = pathname.match(/^\/api\/routines\/([^/]+)\/runs$/);
    if (method === "GET" && routineRunsMatch) {
      return jsonResponse(state.getRoutineRuns(routineRunsMatch[1]));
    }

    const routineTriggerMatch = pathname.match(
      /^\/api\/routines\/([^/]+)\/trigger$/
    );
    if (method === "POST" && routineTriggerMatch) {
      return jsonResponse(state.triggerRoutine(routineTriggerMatch[1]));
    }

    const routineToggleMatch = pathname.match(/^\/api\/routines\/([^/]+)\/toggle$/);
    if (method === "POST" && routineToggleMatch) {
      const body =
        request.headers.get("content-length") === "0"
          ? undefined
          : await request.clone().json().catch(() => undefined as ToggleRequest | undefined);
      return jsonResponse(state.toggleRoutine(routineToggleMatch[1], body));
    }

    if (method === "DELETE" && routineDetailMatch) {
      return jsonResponse(state.deleteRoutine(routineDetailMatch[1]));
    }

    if (method === "GET" && pathname === "/api/extensions") {
      return jsonResponse(state.listExtensions());
    }

    if (method === "GET" && pathname === "/api/extensions/tools") {
      return jsonResponse(state.listExtensionTools());
    }

    if (method === "GET" && pathname === "/api/extensions/registry") {
      return jsonResponse(
        state.searchExtensionRegistry(url.searchParams.get("query"))
      );
    }

    if (method === "POST" && pathname === "/api/extensions/install") {
      const body = await request.json().catch(() => ({ name: "" })) as {
        name?: string;
      };
      if (!body.name) {
        return errorResponse(400, "Missing extension name.");
      }
      return jsonResponse(state.installExtension(body.name));
    }

    const extensionActivateMatch = pathname.match(
      /^\/api\/extensions\/([^/]+)\/activate$/
    );
    if (method === "POST" && extensionActivateMatch) {
      return jsonResponse(state.activateExtension(extensionActivateMatch[1]));
    }

    const extensionRemoveMatch = pathname.match(
      /^\/api\/extensions\/([^/]+)\/remove$/
    );
    if (method === "POST" && extensionRemoveMatch) {
      return jsonResponse(state.removeExtension(extensionRemoveMatch[1]));
    }

    const extensionSetupMatch = pathname.match(
      /^\/api\/extensions\/([^/]+)\/setup$/
    );
    if (extensionSetupMatch && method === "GET") {
      return jsonResponse(state.getExtensionSetup(extensionSetupMatch[1]));
    }

    if (extensionSetupMatch && method === "POST") {
      const body = await parseJson<ExtensionSetupRequest>(request);
      return jsonResponse(
        state.submitExtensionSetup(extensionSetupMatch[1], body.secrets)
      );
    }

    if (method === "GET" && pathname === "/api/skills") {
      return jsonResponse(state.listSkills());
    }

    if (method === "POST" && pathname === "/api/skills/search") {
      const body = await parseJson<SkillSearchRequest>(request);
      return jsonResponse(state.searchSkills(body));
    }

    if (method === "POST" && pathname === "/api/skills/install") {
      const body = await parseJson<SkillInstallRequest>(request);
      return jsonResponse(state.installSkill(body));
    }

    const skillDeleteMatch = pathname.match(/^\/api\/skills\/([^/]+)$/);
    if (method === "DELETE" && skillDeleteMatch) {
      return jsonResponse(state.removeSkill(skillDeleteMatch[1]));
    }

    if (method === "GET" && pathname === "/api/logs/events") {
      return buildLogSseResponse(state);
    }

    if (method === "GET" && pathname === "/api/logs/level") {
      return jsonResponse(state.getLogLevel());
    }

    if (
      (method === "POST" || method === "PUT") &&
      pathname === "/api/logs/level"
    ) {
      const body = await request.json().catch(() => ({ level: "info" })) as {
        level?: string;
      };
      return jsonResponse(state.setLogLevel(body.level ?? "info"));
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
