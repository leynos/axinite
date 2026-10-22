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
import type {
  LogEntry,
  LogLevelRequest,
  LogLevelResponse,
} from "@/lib/api/contracts";
import { isJsonObject, isString } from "@/lib/api/json-value";

/** GET /api/logs/level: fetch the gateway's current runtime log level. */
export function fetchLogLevel(): Promise<LogLevelResponse> {
  return requestJson<LogLevelResponse>("/api/logs/level");
}

/**
 * Set the gateway's runtime log level. The gateway routes this as `PUT`;
 * failures propagate so the caller can reflect the confirmed server value.
 */
export function setLogLevel(level: string): Promise<LogLevelResponse> {
  return putJson<LogLevelResponse, LogLevelRequest>("/api/logs/level", {
    level,
  });
}

function isLogEntry(value: unknown): value is LogEntry {
  if (!isJsonObject(value)) {
    return false;
  }
  const level = value["level"];
  return (
    (level === "trace" ||
      level === "debug" ||
      level === "info" ||
      level === "warn" ||
      level === "error") &&
    isString(value["target"]) &&
    isString(value["message"]) &&
    isString(value["timestamp"])
  );
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
    if (!(rawEvent instanceof MessageEvent) || !isString(rawEvent.data)) {
      return;
    }
    let payload: unknown;
    try {
      payload = JSON.parse(rawEvent.data);
    } catch {
      onError?.();
      return;
    }
    if (!isLogEntry(payload)) {
      onError?.();
      return;
    }
    listener(payload);
  });
  if (onError) {
    setStreamConnectionErrorHandler(source, onError);
  }
  return source;
}
