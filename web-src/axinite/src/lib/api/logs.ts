/**
 * @file Logs API wrapper: the runtime log level at `/api/logs/level` and the
 * `/api/logs/events` SSE tail. Used by the `/logs` surface.
 */

import {
  createEventStream,
  putJson,
  requestJson,
  setStreamConnectionErrorHandler,
} from "@/lib/api/client";
import type { LogEntry, LogLevelResponse } from "@/lib/api/contracts";

/** GET /api/logs/level: fetch the gateway's current runtime log level. */
export function fetchLogLevel(): Promise<LogLevelResponse> {
  return requestJson<LogLevelResponse>("/api/logs/level");
}

/**
 * Set the gateway's runtime log level. The gateway routes this as `PUT`;
 * failures propagate so the caller can reflect the confirmed server value.
 */
export function setLogLevel(level: string): Promise<LogLevelResponse> {
  return putJson<LogLevelResponse>("/api/logs/level", { level });
}

/**
 * GET /api/logs/events: open the log SSE stream and pass each `log` entry to
 * `listener`; malformed frames and stream errors are reported to `onError`.
 */
export function connectLogEvents(
  listener: (entry: LogEntry) => void,
  onError?: () => void
): EventSource {
  const source = createEventStream("/api/logs/events");
  source.addEventListener("log", (rawEvent) => {
    const messageEvent = rawEvent as MessageEvent<unknown>;
    if (typeof messageEvent.data !== "string") {
      return;
    }
    let entry: LogEntry;
    try {
      entry = JSON.parse(messageEvent.data) as LogEntry;
    } catch {
      onError?.();
      return;
    }
    listener(entry);
  });
  if (onError) {
    setStreamConnectionErrorHandler(source, onError);
  }
  return source;
}
