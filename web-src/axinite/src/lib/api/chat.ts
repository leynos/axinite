/**
 * @file Chat API wrapper for the gateway's `/api/chat/*` endpoints: threads,
 * history, sending messages, approvals, and auth tokens, plus the
 * `/api/chat/events` SSE subscription. Used by the `/chat` surface.
 */

import { createEventStream, postJson, requestJson } from "@/lib/api/client";
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
  const url = new URL("/api/chat/history", window.location.origin);
  if (threadId) {
    url.searchParams.set("thread_id", threadId);
  }
  return requestJson<HistoryResponse>(`${url.pathname}${url.search}`);
}

/** POST /api/chat/send: send a chat message. */
export function sendMessage(
  request: SendMessageRequest
): Promise<SendMessageResponse> {
  return postJson<SendMessageResponse>("/api/chat/send", request);
}

/** POST /api/chat/approval: answer a tool approval request. */
export function submitApproval(
  request: ApprovalRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse>("/api/chat/approval", request);
}

/** POST /api/chat/auth-token: submit a token for a pending extension auth. */
export function submitAuthToken(
  request: AuthTokenRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse>("/api/chat/auth-token", request);
}

/** POST /api/chat/auth-cancel: cancel a pending extension auth. */
export function cancelAuth(
  request: AuthCancelRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse>("/api/chat/auth-cancel", request);
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
    source.addEventListener(eventType, (rawEvent) => {
      const messageEvent = rawEvent as MessageEvent<unknown>;
      // The browser dispatches its built-in connection-failure Event under
      // the "error" type too; only gateway frames carry a JSON data string.
      if (typeof messageEvent.data !== "string") {
        return;
      }
      // A malformed frame is reported through `onError` rather than thrown
      // from inside the EventSource callback, where nothing could catch it.
      let event: ChatSseEvent;
      try {
        event = JSON.parse(messageEvent.data) as ChatSseEvent;
      } catch {
        onError?.();
        return;
      }
      listener(event);
    });
  }

  if (onError) {
    source.onerror = () => onError();
  }

  return source;
}
