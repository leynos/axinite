import { createEventStream, postJson, requestJson } from "@/lib/api/client";
import type {
  ActionResponse,
  ApprovalRequest,
  ChatSseEvent,
  HistoryResponse,
  SendMessageRequest,
  SendMessageResponse,
  ThreadInfo,
  ThreadListResponse,
} from "@/lib/api/contracts";

export function fetchThreads(): Promise<ThreadListResponse> {
  return requestJson<ThreadListResponse>("/api/chat/threads");
}

export function createThread(): Promise<ThreadInfo> {
  return postJson<ThreadInfo>("/api/chat/thread/new");
}

export function fetchHistory(
  threadId?: string | null
): Promise<HistoryResponse> {
  const url = new URL("/api/chat/history", window.location.origin);
  if (threadId) {
    url.searchParams.set("thread_id", threadId);
  }
  return requestJson<HistoryResponse>(`${url.pathname}${url.search}`);
}

export function sendMessage(
  request: SendMessageRequest
): Promise<SendMessageResponse> {
  return postJson<SendMessageResponse>("/api/chat/send", request);
}

export function submitApproval(
  request: ApprovalRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse>("/api/chat/approval", request);
}

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
  ];

  for (const eventType of eventTypes) {
    source.addEventListener(eventType, (rawEvent) => {
      const messageEvent = rawEvent as MessageEvent<string>;
      listener(JSON.parse(messageEvent.data) as ChatSseEvent);
    });
  }

  if (onError) {
    source.onerror = () => onError();
  }

  return source;
}
