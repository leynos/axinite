/**
 * @file Compile-time assertions for the front-end API contracts.
 *
 * These exported functions exist for type checking and are never called.
 */

import type { ChatSseEvent, RoutineDetailResponse } from "@/lib/api/contracts";
import type { JsonObject, JsonValue } from "@/lib/api/json-value";
import type { RestartDeps } from "@/lib/restart";

declare const postJson: typeof import("@/lib/api/client").postJson;
declare const putJson: typeof import("@/lib/api/client").putJson;
declare const resolveActiveFilePath: typeof import("@/components/jobs/format").resolveActiveFilePath;

type RestartSendResult = Awaited<ReturnType<RestartDeps["sendRestart"]>>;
type JobToolUseEvent = Extract<ChatSseEvent, { type: "job_tool_use" }>;

export function checkJsonWriteRequestBodies() {
  const post = postJson("/api/chat/send", {
    content: "hello",
    metadata: [null, { source: "type-check" }],
  });
  const put = putJson("/api/logs/level", { level: "info" });

  // @ts-expect-error POST rejects scalar request bodies.
  const scalarPost = postJson("/api/chat/send", "hello");
  // @ts-expect-error PUT rejects scalar request bodies.
  const scalarPut = putJson("/api/logs/level", 2);

  return { post, put, scalarPost, scalarPut };
}

export function checkRestartSendResultVocabulary() {
  const accepted = { kind: "accepted" } satisfies RestartSendResult;
  const transportFailure = {
    kind: "failed",
    category: "transport",
  } satisfies RestartSendResult;
  const acceptedSender: RestartDeps["sendRestart"] = async () => accepted;
  const failedSender: RestartDeps["sendRestart"] = async () => transportFailure;

  // @ts-expect-error Only accepted and failed outcomes are allowed.
  const unknownOutcome = { kind: "rejected" } satisfies RestartSendResult;
  const invalidCategory = {
    kind: "failed",
    // @ts-expect-error Categories are timeout, transport, or HTTP.
    category: "offline",
  } satisfies RestartSendResult;

  return { acceptedSender, failedSender, unknownOutcome, invalidCategory };
}

export function checkRecursiveJsonContracts() {
  const nestedValue = {
    values: [null, false, 4, "text", { deeper: [true] }],
  } satisfies JsonValue;
  const nestedObject = {
    enabled: true,
    options: [{ labels: ["safe", "nested"] }],
  } satisfies JsonObject;

  // @ts-expect-error Undefined is not a JSON value.
  const undefinedValue: JsonValue = { nested: [{ missing: undefined }] };
  // @ts-expect-error BigInt is not a JSON value.
  const bigintValue: JsonValue = { nested: [1n] };

  return { nestedValue, nestedObject, undefinedValue, bigintValue };
}

export function checkChatAndRoutineJsonContracts() {
  const toolUse = {
    type: "job_tool_use",
    job_id: "job-1",
    tool_name: "search",
    input: { query: "example", options: [null, { exact: true }] },
  } satisfies JobToolUseEvent;
  const invalidInput = { callback: () => true };

  const invalidToolUse = {
    ...toolUse,
    // @ts-expect-error Chat tool input must be JSON-compatible.
    input: invalidInput,
  } satisfies JobToolUseEvent;

  const routineDetail = {
    id: "routine-1",
    name: "Daily summary",
    description: "Create a daily summary.",
    enabled: true,
    trigger: { type: "interval", interval: { hours: 24 } },
    action: { type: "agent", parameters: ["summary"] },
    guardrails: { max_steps: 4, allowed: ["read"] },
    notify: { channels: ["web"] },
    last_run_at: null,
    next_fire_at: null,
    run_count: 0,
    consecutive_failures: 0,
    created_at: "2026-10-25T00:00:00Z",
    recent_runs: [],
  } satisfies RoutineDetailResponse;

  const invalidRoutine = {
    ...routineDetail,
    // @ts-expect-error Routine config must be JSON-compatible.
    trigger: { interval: undefined },
  } satisfies RoutineDetailResponse;

  return { toolUse, invalidToolUse, routineDetail, invalidRoutine };
}

export function checkOptionalActiveFileSelection() {
  const fallback = resolveActiveFilePath([]);
  const selected = resolveActiveFilePath([], "selected.md");

  return { fallback, selected } satisfies {
    fallback: string | undefined;
    selected: string | undefined;
  };
}
