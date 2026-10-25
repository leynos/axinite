/**
 * @file Chat API wrapper for the gateway's `/api/chat/*` endpoints: threads,
 * history, sending messages, approvals, and auth tokens, plus the
 * `/api/chat/events` SSE subscription. Used by the `/chat` surface.
 */

import { recordBoundaryFailure } from "@/lib/api/boundary-diagnostics";
import { isChatSseEvent } from "@/lib/api/chat-event-parser";
import {
  createEventStream,
  postJson,
  requestJson,
  setStreamConnectionErrorHandler,
} from "@/lib/api/client";
import type {
  ActionResponse,
  ApprovalRequest,
  AuthCancelRequest,
  AuthTokenRequest,
  ChatSseEvent,
  HistoryResponse,
  SendMessageRequest,
  SendMessageResponse,
  ThreadInfo,
  ThreadListResponse,
} from "@/lib/api/contracts";
import { isString } from "@/lib/api/json-value";

/** GET /api/chat/threads: list chat threads. */
export function fetchThreads(): Promise<ThreadListResponse> {
  return requestJson<ThreadListResponse>("/api/chat/threads");
}

/** POST /api/chat/thread/new: create a new chat thread. */
export function createThread(): Promise<ThreadInfo> {
  return postJson<ThreadInfo>("/api/chat/thread/new");
}

/** GET /api/chat/history: fetch a thread's history (`thread_id` optional). */
export function fetchHistory(
  threadId?: string | null
): Promise<HistoryResponse> {
  const url = new URL("/api/chat/history", globalThis.location.origin);
  if (threadId) {
    url.searchParams.set("thread_id", threadId);
  }
  return requestJson<HistoryResponse>(`${url.pathname}${url.search}`);
}

/** POST /api/chat/send: send a chat message. */
export function sendMessage(
  request: SendMessageRequest
): Promise<SendMessageResponse> {
  return postJson<SendMessageResponse, SendMessageRequest>(
    "/api/chat/send",
    request
  );
}

/** POST /api/chat/approval: answer a tool approval request. */
export function submitApproval(
  request: ApprovalRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse, ApprovalRequest>(
    "/api/chat/approval",
    request
  );
}

/** POST /api/chat/auth-token: submit a token for a pending extension auth. */
export function submitAuthToken(
  request: AuthTokenRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse, AuthTokenRequest>(
    "/api/chat/auth-token",
    request
  );
}

/** POST /api/chat/auth-cancel: cancel a pending extension auth. */
export function cancelAuth(
  request: AuthCancelRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse, AuthCancelRequest>(
    "/api/chat/auth-cancel",
    request
  );
}

function reportUnexpectedChatEvent(
  eventType: ChatSseEvent["type"],
  onError?: () => void
): void {
  // EventSource's native error event is reported by the shared transport hook.
  if (eventType === "error") {
    return;
  }
  recordBoundaryFailure("chat-stream", "invalid-payload");
  onError?.();
}

function handleChatEventFrame(
  eventType: ChatSseEvent["type"],
  rawEvent: Event,
  listener: (event: ChatSseEvent) => void,
  onError?: () => void
): void {
  if (!(rawEvent instanceof MessageEvent)) {
    reportUnexpectedChatEvent(eventType, onError);
    return;
  }
  if (!isString(rawEvent.data)) {
    recordBoundaryFailure("chat-stream", "invalid-payload");
    onError?.();
    return;
  }

  // Parse errors stay inside this callback and reach the stream error hook.
  let payload: unknown;
  try {
    payload = JSON.parse(rawEvent.data);
  } catch {
    recordBoundaryFailure("chat-stream", "invalid-json");
    onError?.();
    return;
  }
  if (!isChatSseEvent(payload)) {
    recordBoundaryFailure("chat-stream", "invalid-payload");
    onError?.();
    return;
  }
  listener(payload);
}

/**
 * GET /api/chat/events: open the chat SSE stream and pass each gateway event
 * to `listener`; malformed frames and stream errors are reported to `onError`.
 */
export function connectChatEvents(
  listener: (event: ChatSseEvent) => void,
  onError?: () => void
): EventSource {
  const source = createEventStream("/api/chat/events");
  const eventTypes: ChatSseEvent["type"][] = [
    "response",
    "thinking",
    "tool_started",
    "tool_completed",
    "tool_result",
    "stream_chunk",
    "status",
    "approval_needed",
    "auth_required",
    "auth_completed",
    "error",
    "heartbeat",
    "extension_status",
    "job_started",
    "job_message",
    "job_tool_use",
    "job_tool_result",
    "job_status",
    "job_result",
    "image_generated",
  ];

  for (const eventType of eventTypes) {
    source.addEventListener(eventType, (rawEvent) =>
      handleChatEventFrame(eventType, rawEvent, listener, onError)
    );
  }

  setStreamConnectionErrorHandler(source, () => {
    recordBoundaryFailure("chat-stream", "connection");
    onError?.();
  });

  return source;
}
