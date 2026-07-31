import type {
  ActionResponse,
  ApprovalRequest,
  ChatSseEvent,
  LogEntry,
  RoutineDetailResponse,
  SendMessageRequest,
  SkillInstallRequest,
  TurnInfo,
} from "../../axinite/src/lib/api/contracts";
import type { MockCatalogueSkill, MockThread } from "./fixtures";

// Shared preview string reused by the tool_result event and the recorded
// tool-call result on a chat turn.
const TOOL_RESULT_PREVIEW =
  "Collected current route state, mock transport health, and feature-flag visibility.";

export function nowIso(): string {
  return new Date().toISOString();
}

export function createActionResponse(
  message: string,
  overrides: Partial<ActionResponse> = {}
): ActionResponse {
  return {
    success: true,
    message,
    ...overrides,
  };
}

export function statusFromRoutine(detail: RoutineDetailResponse): string {
  if (!detail.enabled) {
    return "disabled";
  }
  if (detail.consecutive_failures > 0) {
    return "failing";
  }
  return "active";
}

// Context passed to the chat-turn emission timeline. The mock backend supplies
// bound `publish`/`pushLog` callbacks plus the mutable turn and thread objects
// so the scheduled emissions mutate exactly the same state the class owns.
export type ChatEmitContext = {
  request: SendMessageRequest;
  thread: MockThread;
  turn: TurnInfo;
  imageDataUrl: string;
  publish: (event: ChatSseEvent) => void;
  pushLog: (message: string, target: string, level?: LogEntry["level"]) => void;
  now: () => string;
};

// Derived per-turn plan the emission schedule operates on. Folding the derived
// primitives (restart flag, tool name, response chunks) into one object keeps
// each schedule step a single-object function.
type ChatTurnPlan = ChatEmitContext & {
  lowerContent: string;
  isRestart: boolean;
  toolName: string;
  chunks: string[];
};

function resolveToolName(plan: ChatEmitContext & { isRestart: boolean }): string {
  if (plan.isRestart) {
    return "restart";
  }
  const lowerContent = plan.request.content.toLowerCase();
  if (lowerContent.includes("log") || lowerContent.includes("inspect")) {
    return "inspect_preview_stack";
  }
  return "write_preview_summary";
}

function scheduleJobStarted(plan: ChatTurnPlan): void {
  setTimeout(() => {
    plan.publish({
      type: "job_started",
      job_id: "job-spawned-1",
      title: "Spawned preview job",
      browse_url: "/projects/job-spawned-1/",
    });
  }, 60);
}

function scheduleToolStarted(plan: ChatTurnPlan): void {
  setTimeout(() => {
    plan.turn.tool_calls = [
      { name: plan.toolName, has_result: false, has_error: false },
    ];
    plan.publish({
      type: "tool_started",
      name: plan.toolName,
      thread_id: plan.thread.info.id,
    });
  }, 120);
}

function scheduleToolResult(plan: ChatTurnPlan): void {
  setTimeout(() => {
    plan.publish({
      type: "tool_result",
      name: plan.toolName,
      preview: TOOL_RESULT_PREVIEW,
      thread_id: plan.thread.info.id,
    });
    plan.turn.tool_calls = [
      {
        name: plan.toolName,
        has_result: true,
        has_error: false,
        result_preview: TOOL_RESULT_PREVIEW,
      },
    ];
  }, 260);
}

function scheduleToolCompleted(plan: ChatTurnPlan): void {
  setTimeout(() => {
    plan.publish({
      type: "tool_completed",
      name: plan.toolName,
      success: true,
      thread_id: plan.thread.info.id,
    });
    if (plan.isRestart) {
      plan.turn.tool_calls = [
        {
          name: plan.toolName,
          has_result: true,
          has_error: false,
          result_preview: "Restart sequence acknowledged.",
        },
      ];
    }
  }, 340);
}

function scheduleImageGenerated(plan: ChatTurnPlan): void {
  setTimeout(() => {
    plan.publish({
      type: "image_generated",
      data_url: plan.imageDataUrl,
      path: "workspace/generated/preview.png",
      thread_id: plan.thread.info.id,
    });
  }, 360);
}

function buildResponseParts(
  plan: ChatEmitContext & { isRestart: boolean }
): string[] {
  if (plan.isRestart) {
    return [
      "Restart initiated. ",
      "The mock preview backend acknowledged the /restart command.",
    ];
  }
  const imageCount = plan.request.images?.length ?? 0;
  return [
    "Mock backend response for ",
    `"${plan.request.content}": `,
    "the preview is now wired to typed JSON and SSE routes ",
    "instead of local screen fixture arrays.",
    ...(imageCount > 0
      ? [` Received ${imageCount} image attachment(s).`]
      : []),
  ];
}

function scheduleResponseStream(plan: ChatTurnPlan): void {
  plan.chunks.forEach((content, index) => {
    setTimeout(() => {
      plan.publish({
        type: "stream_chunk",
        content,
        thread_id: plan.thread.info.id,
      });
    }, 420 + index * 70);
  });
}

function scheduleCompletion(plan: ChatTurnPlan): void {
  const fullResponse = plan.chunks.join("");
  setTimeout(() => {
    plan.turn.response = fullResponse;
    plan.turn.state = "Completed";
    plan.turn.completed_at = plan.now();
    plan.thread.info = {
      ...plan.thread.info,
      state: "Idle",
      turn_count: plan.thread.turns.length,
      updated_at: plan.turn.completed_at,
    };
    plan.publish({
      type: "response",
      content: fullResponse,
      thread_id: plan.thread.info.id,
    });
    plan.pushLog(
      `Completed streamed response for ${plan.thread.info.id}.`,
      "chat"
    );
  }, 820);
}

function planChatTurn(ctx: ChatEmitContext): ChatTurnPlan {
  const lowerContent = ctx.request.content.toLowerCase();
  const isRestart = ctx.request.content === "/restart";
  const withRestart = { ...ctx, isRestart };
  return {
    ...ctx,
    lowerContent,
    isRestart,
    toolName: resolveToolName(withRestart),
    chunks: buildResponseParts(withRestart),
  };
}

// Schedule the full trigger-specific emission timeline for one chat turn. The
// setTimeout delays and payloads are byte-identical to the previous inline
// implementation so the mock-backend contract ordering stays pinned.
export function emitChatTurnSequence(ctx: ChatEmitContext): void {
  const plan = planChatTurn(ctx);

  if (plan.lowerContent.includes("job")) {
    scheduleJobStarted(plan);
  }
  scheduleToolStarted(plan);
  if (!plan.isRestart) {
    scheduleToolResult(plan);
  }
  scheduleToolCompleted(plan);
  if (!plan.isRestart && plan.lowerContent.includes("image")) {
    scheduleImageGenerated(plan);
  }

  scheduleResponseStream(plan);
  scheduleCompletion(plan);
}

export type ApprovalOutcome = {
  eventType: "error" | "status";
  message: string;
  logLevel: LogEntry["level"];
  response: string;
};

export type ApprovalDecision = {
  isDeny: boolean;
  action: string;
  toolName: string;
};

// Collapse the deny/approve branching for one approval decision into a single
// value so the caller needs no per-field conditionals.
export function buildApprovalOutcome(
  decision: ApprovalDecision
): ApprovalOutcome {
  if (decision.isDeny) {
    return {
      eventType: "error",
      message: `Denied ${decision.toolName}.`,
      logLevel: "warn",
      response: "Request denied.",
    };
  }
  return {
    eventType: "status",
    message: `${decision.action} confirmed for ${decision.toolName}.`,
    logLevel: "info",
    response: "Approval recorded and the conversation resumed.",
  };
}

export function approvalActionLabel(action: ApprovalRequest["action"]): string {
  if (action === "always") {
    return "always approve";
  }
  if (action === "deny") {
    return "deny";
  }
  return "approve";
}

// Record the approval decision on the most recent turn of the thread. A denied
// action fails the turn; any other action completes it. Absent turns are a
// no-op, matching the previous inline guard.
export function applyApprovalToLatestTurn(params: {
  thread: MockThread;
  isDeny: boolean;
  now: () => string;
}): void {
  const latestTurn = params.thread.turns.at(-1);
  if (!latestTurn) {
    return;
  }
  latestTurn.state = params.isDeny ? "Failed" : "Completed";
  latestTurn.completed_at = params.now();
  latestTurn.response = params.isDeny
    ? "The pending action was denied, so no file changes were applied."
    : "Approval received. Proceeding with the approved mock backend implementation slice.";
}

export function resolveSkillSource(request: SkillInstallRequest): string {
  if (request.url) {
    return "url";
  }
  if (request.content) {
    return "content";
  }
  return "catalog";
}

export function matchCatalogueSkill(
  entries: MockCatalogueSkill[],
  request: SkillInstallRequest
): MockCatalogueSkill | null {
  return (
    entries.find(
      (entry) =>
        entry.name === request.name ||
        entry.slug === request.slug ||
        entry.slug.endsWith(`/${request.name}`)
    ) ?? null
  );
}
