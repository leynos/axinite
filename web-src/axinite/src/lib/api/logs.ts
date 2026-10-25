/**
 * @file Logs API wrapper: the runtime log level at `/api/logs/level` and the
 * `/api/logs/events` SSE tail. Used by the `/logs` surface.
 */

import { recordBoundaryFailure } from "@/lib/api/boundary-diagnostics";
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
import {
  isJsonObject,
  isString,
  type JsonObject,
  type JsonValue,
} from "@/lib/api/json-value";

const LOG_LEVELS = new Map<string, LogEntry["level"]>([
  ["trace", "trace"],
  ["debug", "debug"],
  ["info", "info"],
  ["warn", "warn"],
  ["error", "error"],
]);

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

function parseLogEntry(value: JsonObject): LogEntry | undefined {
  const level = parseLogLevel(value["level"]);
  if (level === undefined) {
    return undefined;
  }
  const target = value["target"];
  if (!isString(target)) {
    return undefined;
  }
  const message = value["message"];
  if (!isString(message)) {
    return undefined;
  }
  const timestamp = value["timestamp"];
  if (!isString(timestamp)) {
    return undefined;
  }
  return { level, target, message, timestamp };
}

function parseLogLevel(
  value: JsonValue | undefined
): LogEntry["level"] | undefined {
  if (!isString(value)) {
    return undefined;
  }
  return LOG_LEVELS.get(value.toLowerCase());
}

type LogEventFailure = "invalid-json" | "invalid-payload";

type LogEventParseResult =
  | { status: "parsed"; entry: LogEntry }
  | { status: "rejected"; reason: LogEventFailure };

function rejectLogEvent(reason: LogEventFailure, onError?: () => void): void {
  recordBoundaryFailure("log-stream", reason);
  onError?.();
}

function parseLogEventPayload(serializedJson: string): LogEventParseResult {
  let payload: unknown;
  try {
    payload = JSON.parse(serializedJson);
  } catch {
    return { status: "rejected", reason: "invalid-json" };
  }
  if (!isJsonObject(payload)) {
    return { status: "rejected", reason: "invalid-payload" };
  }
  const entry = parseLogEntry(payload);
  return entry === undefined
    ? { status: "rejected", reason: "invalid-payload" }
    : { status: "parsed", entry };
}

function parseLogEvent(
  rawEvent: Event,
  onError?: () => void
): LogEntry | undefined {
  if (!(rawEvent instanceof MessageEvent)) {
    rejectLogEvent("invalid-payload", onError);
    return undefined;
  }
  const result = parseLogEventPayload(rawEvent.data);
  if (result.status === "rejected") {
    rejectLogEvent(result.reason, onError);
    return undefined;
  }
  return result.entry;
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
    const entry = parseLogEvent(rawEvent, onError);
    if (entry === undefined) {
      return;
    }
    listener(entry);
  });
  setStreamConnectionErrorHandler(source, () => {
    recordBoundaryFailure("log-stream", "connection");
    onError?.();
  });
  return source;
}
