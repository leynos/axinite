import { createEventStream, postJson, requestJson } from "@/lib/api/client";
import type { LogEntry, LogLevelResponse } from "@/lib/api/contracts";

export function fetchLogLevel(): Promise<LogLevelResponse> {
  return requestJson<LogLevelResponse>("/api/logs/level").catch(() => ({
    level: "info",
  }));
}

export function setLogLevel(level: string): Promise<LogLevelResponse> {
  return postJson<LogLevelResponse>("/api/logs/level", { level }).catch(() => ({
    level,
  }));
}

export function connectLogEvents(
  listener: (entry: LogEntry) => void,
  onError?: () => void
): EventSource {
  const source = createEventStream("/api/logs/events");
  source.addEventListener("log", (rawEvent) => {
    const messageEvent = rawEvent as MessageEvent<string>;
    listener(JSON.parse(messageEvent.data) as LogEntry);
  });
  if (onError) {
    source.onerror = () => onError();
  }
  return source;
}
