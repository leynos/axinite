/**
 * @file Contract tests for the daemon-free mock backend request handler and
 * state, exercised in-process through `handleMockRequest` rather than a socket.
 */

import { describe, expect, it } from "vitest";

import type {
  ExtensionListResponse,
  FeatureFlagsResponse,
  GatewayStatusResponse,
  JobListResponse,
  MemoryTreeResponse,
  PairingListResponse,
  RoutineListResponse,
  SkillListResponse,
  ThreadListResponse,
} from "@/lib/api/contracts";
import { isBoolean, isJsonObject, type JsonObject } from "@/lib/api/json-value";
import {
  handleMockRequest,
  parseFailureRoutes,
} from "../../mock-backend/src/server";
import { MockBackendState } from "../../mock-backend/src/state";

// Contract tests for the daemon-free stub runtime: every route the SolidJS
// app calls during initial load and its key flows must answer with the
// gateway-shaped payloads, and the SSE routes must emit parseable frames in
// a deterministic order.

function request(path: string, init?: RequestInit): Request {
  return new Request(`http://mock.test${path}`, init);
}

function parseJsonObject(text: string): JsonObject {
  const value: unknown = JSON.parse(text);
  if (!isJsonObject(value)) {
    throw new Error("Expected a JSON object in the mock backend response");
  }
  return value;
}

async function readJsonObject(response: Response): Promise<JsonObject> {
  const value: unknown = await response.json();
  if (!isJsonObject(value)) {
    throw new Error("Expected a JSON object in the mock backend response");
  }
  return value;
}

async function getJson<T>(state: MockBackendState, path: string): Promise<T> {
  const response = await handleMockRequest(request(path), state);
  expect(response.status).toBe(200);
  expect(response.headers.get("content-type")).toContain("application/json");
  // SAFETY: The route-specific type at each call site matches the typed DTO
  // returned by this in-process mock backend handler.
  return (await response.json()) as T;
}

type SseFrame = { event: string; data: string };

function parseSseFrames(buffer: string): SseFrame[] {
  return buffer
    .split("\n\n")
    .filter((block) => block.includes("event:"))
    .map((block) => {
      const event = /event: (.+)/.exec(block)?.[1] ?? "";
      const data = /data: (.+)/.exec(block)?.[1] ?? "";
      return { event, data };
    });
}

async function collectSse(
  response: Response,
  isDone: (frames: SseFrame[]) => boolean,
  timeoutMs = 3000
): Promise<SseFrame[]> {
  const reader = response.body?.getReader();
  expect(reader).toBeDefined();
  if (!reader) {
    return [];
  }
  const decoder = new TextDecoder();
  let buffer = "";
  const deadline = Date.now() + timeoutMs;
  try {
    while (Date.now() < deadline) {
      const race = await Promise.race([
        reader.read(),
        new Promise<"timeout">((resolve) =>
          setTimeout(() => resolve("timeout"), deadline - Date.now())
        ),
      ]);
      if (race === "timeout") {
        break;
      }
      if (race.done) {
        break;
      }
      buffer += decoder.decode(race.value, { stream: true });
      if (isDone(parseSseFrames(buffer))) {
        break;
      }
    }
  } finally {
    await reader.cancel();
  }
  return parseSseFrames(buffer);
}

describe("stub HTTP contract", () => {
  it.each([
    ["malformed JSON", "{ malformed"],
    ["an invalid schema", JSON.stringify({ enabled: "false" })],
  ])("rejects routine toggle bodies with %s without toggling", async (_, body) => {
    const state = new MockBackendState();
    try {
      const routineBefore = state.getRoutine("routine-standup");
      const response = await handleMockRequest(
        request("/api/routines/routine-standup/toggle", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body,
        }),
        state
      );

      expect(response.status).toBe(400);
      expect(await response.json()).toEqual({
        error: "Invalid routine toggle body.",
      });
      expect(state.getRoutine("routine-standup")).toEqual(routineBefore);
    } finally {
      state.dispose();
    }
  });

  it("allows a bodyless routine toggle", async () => {
    const state = new MockBackendState();
    try {
      const enabledBefore = state.getRoutine("routine-standup").enabled;
      const response = await handleMockRequest(
        request("/api/routines/routine-standup/toggle", { method: "POST" }),
        state
      );

      expect(response.status).toBe(200);
      expect(state.getRoutine("routine-standup").enabled).toBe(!enabledBefore);
    } finally {
      state.dispose();
    }
  });

  it("rejects malformed chat bodies before changing mock state", async () => {
    const state = new MockBackendState();
    const threadsBefore = state.listThreads();
    const historyBefore = state.getHistory(null);
    const response = await handleMockRequest(
      request("/api/chat/send", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ content: "hello", images: [{ data: "AA==" }] }),
      }),
      state
    );

    expect(response.status).toBe(400);
    expect(await response.json()).toMatchObject({
      error: "Invalid chat message body.",
    });
    expect(state.listThreads()).toEqual(threadsBefore);
    expect(state.getHistory(null)).toEqual(historyBefore);
  });

  it("serves the feature-flag map as flag names to booleans", async () => {
    const flags = await getJson<FeatureFlagsResponse>(
      new MockBackendState(),
      "/api/features"
    );

    expect(flags["route_chat"]).toBe(true);
    expect(flags["panel_logs"]).toBe(true);
    expect(flags["action_memory_edit"]).toBe(false);
    for (const value of Object.values(flags)) {
      expect(isBoolean(value)).toBe(true);
    }
  });

  it("honours injected feature-flag overrides", async () => {
    const flags = await getJson<FeatureFlagsResponse>(
      new MockBackendState(undefined, {
        featureFlagOverrides: { panel_logs: false },
      }),
      "/api/features"
    );
    expect(flags["panel_logs"]).toBe(false);
  });

  it("serves gateway status with the operational telemetry fields", async () => {
    const status = await getJson<GatewayStatusResponse>(
      new MockBackendState(),
      "/api/gateway/status"
    );
    expect(status).toMatchObject({
      version: expect.any(String),
      sse_connections: expect.any(Number),
      uptime_secs: expect.any(Number),
    });
  });

  it("serves the initial-load list routes with their expected shapes", async () => {
    const state = new MockBackendState();

    const threads = await getJson<ThreadListResponse>(
      state,
      "/api/chat/threads"
    );
    expect(Array.isArray(threads.threads)).toBe(true);

    const jobs = await getJson<JobListResponse>(state, "/api/jobs");
    expect(Array.isArray(jobs.jobs)).toBe(true);

    const routines = await getJson<RoutineListResponse>(state, "/api/routines");
    expect(Array.isArray(routines.routines)).toBe(true);

    const extensions = await getJson<ExtensionListResponse>(
      state,
      "/api/extensions"
    );
    expect(Array.isArray(extensions.extensions)).toBe(true);

    const skills = await getJson<SkillListResponse>(state, "/api/skills");
    expect(Array.isArray(skills.skills)).toBe(true);

    const tree = await getJson<MemoryTreeResponse>(state, "/api/memory/tree");
    expect(Array.isArray(tree.entries)).toBe(true);
  });

  it("returns a deterministic failure fixture for configured routes", async () => {
    const failures = parseFailureRoutes("/api/jobs, /api/skills");
    const response = await handleMockRequest(
      request("/api/jobs"),
      new MockBackendState(),
      failures
    );
    expect(response.status).toBe(500);
    const body: { error: string } = await response.json();
    expect(body.error).toContain("/api/jobs");

    const untouched = await handleMockRequest(
      request("/api/routines"),
      new MockBackendState(),
      failures
    );
    expect(untouched.status).toBe(200);
  });

  it("rejects unknown routes with 404", async () => {
    const response = await handleMockRequest(
      request("/api/unknown"),
      new MockBackendState()
    );
    expect(response.status).toBe(404);
  });
});

describe("stub SSE contract", () => {
  it("streams log replay as event-stream frames with gateway-shaped entries", async () => {
    const state = new MockBackendState();
    const response = await handleMockRequest(
      request("/api/logs/events"),
      state
    );

    expect(response.status).toBe(200);
    expect(response.headers.get("content-type")).toBe("text/event-stream");
    expect(response.headers.get("cache-control")).toBe("no-cache");

    const frames = await collectSse(response, (seen) => seen.length >= 4, 2000);
    expect(frames.length).toBeGreaterThanOrEqual(4);
    for (const frame of frames) {
      expect(frame.event).toBe("log");
      const entry = parseJsonObject(frame.data);
      expect(entry).toMatchObject({
        level: expect.any(String),
        target: expect.any(String),
        message: expect.any(String),
        timestamp: expect.any(String),
      });
    }
  });

  it("emits the chat turn lifecycle in a deterministic order", async () => {
    const state = new MockBackendState();
    const response = await handleMockRequest(
      request("/api/chat/events"),
      state
    );
    expect(response.headers.get("content-type")).toBe("text/event-stream");

    const send = await handleMockRequest(
      request("/api/chat/send", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          content: "Summarize the preview state",
          images: [],
        }),
      }),
      state
    );
    expect(send.status).toBe(202);

    const frames = await collectSse(response, (seen) =>
      seen.some((frame) => frame.event === "response")
    );

    const order = frames.map((frame) => frame.event);
    const thinking = order.indexOf("thinking");
    const toolStarted = order.indexOf("tool_started");
    const responseIndex = order.indexOf("response");
    expect(thinking).toBeGreaterThanOrEqual(0);
    expect(toolStarted).toBeGreaterThan(thinking);
    expect(responseIndex).toBeGreaterThan(toolStarted);

    for (const frame of frames) {
      const payload = parseJsonObject(frame.data);
      expect(payload["type"]).toBe(frame.event);
    }
  });

  it("emits a restart sequence with a named restart tool before the response", async () => {
    const state = new MockBackendState();
    const response = await handleMockRequest(
      request("/api/chat/events"),
      state
    );

    const send = await handleMockRequest(
      request("/api/chat/send", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ content: "/restart", images: [] }),
      }),
      state
    );
    expect(send.status).toBe(202);

    const frames = await collectSse(response, (seen) =>
      seen.some((frame) => frame.event === "response")
    );
    const order = frames.map((frame) => frame.event);
    const toolCompletedIndex = order.findIndex((event, index) => {
      if (event !== "tool_completed") {
        return false;
      }
      const payload = parseJsonObject(frames[index]?.data ?? "null");
      return payload["name"] === "restart";
    });
    const responseIndex = order.indexOf("response");
    expect(toolCompletedIndex).toBeGreaterThanOrEqual(0);
    expect(responseIndex).toBeGreaterThan(toolCompletedIndex);

    const toolStartedFrame = frames[order.indexOf("tool_started")];
    expect(parseJsonObject(toolStartedFrame?.data ?? "null")).toMatchObject({
      name: "restart",
    });

    const responseFrame = frames[responseIndex];
    expect(parseJsonObject(responseFrame?.data ?? "null")).toMatchObject({
      content: expect.stringContaining("Restart initiated"),
    });
  });

  it("emits image_generated with an inline data URL for image-flavoured prompts", async () => {
    const state = new MockBackendState();
    const response = await handleMockRequest(
      request("/api/chat/events"),
      state
    );

    const send = await handleMockRequest(
      request("/api/chat/send", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          content: "Generate an image of a robot panda",
          images: [],
        }),
      }),
      state
    );
    expect(send.status).toBe(202);

    const frames = await collectSse(response, (seen) =>
      seen.some((frame) => frame.event === "response")
    );

    const imageFrame = frames.find(
      (frame) => frame.event === "image_generated"
    );
    expect(imageFrame).toBeDefined();
    const payload = parseJsonObject(imageFrame?.data ?? "{}");
    expect(payload["data_url"]).toMatch(/^data:image\/png;base64,/);
    expect(payload["path"]).toBe("workspace/generated/preview.png");
  });

  it("emits job_started for job-flavoured prompts", async () => {
    const state = new MockBackendState();
    const response = await handleMockRequest(
      request("/api/chat/events"),
      state
    );

    const send = await handleMockRequest(
      request("/api/chat/send", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          content: "Spawn a background job for this",
          images: [],
        }),
      }),
      state
    );
    expect(send.status).toBe(202);

    const frames = await collectSse(response, (seen) =>
      seen.some((frame) => frame.event === "response")
    );

    const jobFrame = frames.find((frame) => frame.event === "job_started");
    expect(jobFrame).toBeDefined();
    expect(parseJsonObject(jobFrame?.data ?? "{}")).toMatchObject({
      job_id: "job-spawned-1",
      title: "Spawned preview job",
      browse_url: "/projects/job-spawned-1/",
    });
  });

  it("acknowledges attached images in the canned response text", async () => {
    const state = new MockBackendState();
    const response = await handleMockRequest(
      request("/api/chat/events"),
      state
    );

    const send = await handleMockRequest(
      request("/api/chat/send", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          content: "Here are two photos",
          images: [
            { media_type: "image/png", data: "AA==" },
            { media_type: "image/png", data: "AA==" },
          ],
        }),
      }),
      state
    );
    expect(send.status).toBe(202);

    const frames = await collectSse(response, (seen) =>
      seen.some((frame) => frame.event === "response")
    );
    const responseFrame = frames.find((frame) => frame.event === "response");
    expect(responseFrame).toBeDefined();
    const payload = parseJsonObject(responseFrame?.data ?? "{}");
    expect(payload["content"]).toContain("Received 2 image attachment(s).");
  });

  it("publishes auth_completed on the chat stream when a chat auth token succeeds", async () => {
    const state = new MockBackendState();
    const response = await handleMockRequest(
      request("/api/chat/events"),
      state
    );

    const authResponse = await handleMockRequest(
      request("/api/chat/auth-token", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          extension_name: "google-drive",
          token: "valid-token",
        }),
      }),
      state
    );
    expect(authResponse.status).toBe(200);
    const authBody = await readJsonObject(authResponse);
    expect(authBody["success"]).toBe(true);

    const frames = await collectSse(response, (seen) =>
      seen.some((frame) => frame.event === "auth_completed")
    );
    const authFrame = frames.find((frame) => frame.event === "auth_completed");
    expect(authFrame).toBeDefined();
    expect(parseJsonObject(authFrame?.data ?? "{}")).toMatchObject({
      type: "auth_completed",
      extension_name: "google-drive",
      success: true,
      message: "Authentication completed.",
    });
  });

  it("rejects a chat auth token that is too short", async () => {
    const state = new MockBackendState();
    const response = await handleMockRequest(
      request("/api/chat/auth-token", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ extension_name: "google-drive", token: "abc" }),
      }),
      state
    );
    expect(response.status).toBe(200);
    const body = await readJsonObject(response);
    expect(body["success"]).toBe(false);
  });

  it("cancels a chat auth flow", async () => {
    const state = new MockBackendState();
    const response = await handleMockRequest(
      request("/api/chat/auth-cancel", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ extension_name: "google-drive" }),
      }),
      state
    );
    expect(response.status).toBe(200);
    const body = await readJsonObject(response);
    expect(body["success"]).toBe(true);
  });
});

describe("stub pairing contract", () => {
  it("returns an empty request list for an unknown channel", async () => {
    const state = new MockBackendState();
    const list = await getJson<PairingListResponse>(
      state,
      "/api/pairing/does-not-exist"
    );
    expect(list.channel).toBe("does-not-exist");
    expect(list.requests).toEqual([]);
  });

  it("lists the deterministic whatsapp pending pairing request", async () => {
    const state = new MockBackendState();
    const list = await getJson<PairingListResponse>(
      state,
      "/api/pairing/whatsapp"
    );
    expect(list.requests).toHaveLength(1);
    expect(list.requests[0]).toMatchObject({
      code: "PAIR-1234",
      sender_id: "user-42",
    });
  });

  it("approves a known pairing code and removes it from the list", async () => {
    const state = new MockBackendState();
    const approve = await handleMockRequest(
      request("/api/pairing/whatsapp/approve", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ code: "PAIR-1234" }),
      }),
      state
    );
    expect(approve.status).toBe(200);
    const body = await readJsonObject(approve);
    expect(body["success"]).toBe(true);

    const list = await getJson<PairingListResponse>(
      state,
      "/api/pairing/whatsapp"
    );
    expect(list.requests).toEqual([]);
  });

  it("returns success: false with 200 for an unknown pairing code", async () => {
    const state = new MockBackendState();
    const approve = await handleMockRequest(
      request("/api/pairing/whatsapp/approve", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ code: "NOT-A-CODE" }),
      }),
      state
    );
    expect(approve.status).toBe(200);
    const body = await readJsonObject(approve);
    expect(body["success"]).toBe(false);
  });

  it("returns a plain-text 429 for the rate-limited fixture code", async () => {
    const state = new MockBackendState();
    const approve = await handleMockRequest(
      request("/api/pairing/whatsapp/approve", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ code: "rate-limited" }),
      }),
      state
    );
    expect(approve.status).toBe(429);
    expect(approve.headers.get("content-type")).toContain("text/plain");
    const text = await approve.text();
    expect(text).toBe("Too many failed approve attempts; try again later");
  });
});
