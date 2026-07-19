import { afterEach, describe, expect, it } from "vitest";

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

async function getJson(
  state: MockBackendState,
  path: string
): Promise<unknown> {
  const response = await handleMockRequest(request(path), state);
  expect(response.status).toBe(200);
  expect(response.headers.get("content-type")).toContain("application/json");
  return response.json();
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
  timeoutMs = 3_000
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

afterEach(() => {
  delete process.env.FEATURE_FLAG_PANEL_LOGS;
});

describe("stub HTTP contract", () => {
  it("serves the feature-flag map as flag names to booleans", async () => {
    const flags = (await getJson(
      new MockBackendState(),
      "/api/features"
    )) as Record<string, unknown>;

    expect(flags.route_chat).toBe(true);
    expect(flags.panel_logs).toBe(true);
    expect(flags.action_memory_edit).toBe(false);
    for (const value of Object.values(flags)) {
      expect(typeof value).toBe("boolean");
    }
  });

  it("honours FEATURE_FLAG environment overrides", async () => {
    process.env.FEATURE_FLAG_PANEL_LOGS = "false";
    const flags = (await getJson(
      new MockBackendState(),
      "/api/features"
    )) as Record<string, unknown>;
    expect(flags.panel_logs).toBe(false);
  });

  it("serves gateway status with the operational telemetry fields", async () => {
    const status = (await getJson(
      new MockBackendState(),
      "/api/gateway/status"
    )) as Record<string, unknown>;
    expect(status).toMatchObject({
      version: expect.any(String),
      sse_connections: expect.any(Number),
      uptime_secs: expect.any(Number),
    });
  });

  it("serves the initial-load list routes with their expected shapes", async () => {
    const state = new MockBackendState();

    const threads = (await getJson(state, "/api/chat/threads")) as {
      threads: unknown[];
    };
    expect(Array.isArray(threads.threads)).toBe(true);

    const jobs = (await getJson(state, "/api/jobs")) as { jobs: unknown[] };
    expect(Array.isArray(jobs.jobs)).toBe(true);

    const routines = (await getJson(state, "/api/routines")) as {
      routines: unknown[];
    };
    expect(Array.isArray(routines.routines)).toBe(true);

    const extensions = (await getJson(state, "/api/extensions")) as {
      extensions: unknown[];
    };
    expect(Array.isArray(extensions.extensions)).toBe(true);

    const skills = (await getJson(state, "/api/skills")) as {
      skills: unknown[];
    };
    expect(Array.isArray(skills.skills)).toBe(true);

    const tree = (await getJson(state, "/api/memory/tree")) as {
      entries: unknown[];
    };
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
    const body = (await response.json()) as { error: string };
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

    const frames = await collectSse(
      response,
      (seen) => seen.length >= 4,
      2_000
    );
    expect(frames.length).toBeGreaterThanOrEqual(4);
    for (const frame of frames) {
      expect(frame.event).toBe("log");
      const entry = JSON.parse(frame.data) as Record<string, unknown>;
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
        body: JSON.stringify({ content: "Summarize the preview state" }),
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
      const payload = JSON.parse(frame.data) as { type: string };
      expect(payload.type).toBe(frame.event);
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
        body: JSON.stringify({ content: "/restart" }),
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
      const payload = JSON.parse(frames[index].data) as { name: string };
      return payload.name === "restart";
    });
    const responseIndex = order.indexOf("response");
    expect(toolCompletedIndex).toBeGreaterThanOrEqual(0);
    expect(responseIndex).toBeGreaterThan(toolCompletedIndex);

    const toolStartedFrame = frames[order.indexOf("tool_started")];
    expect(JSON.parse(toolStartedFrame.data)).toMatchObject({
      name: "restart",
    });

    const responseFrame = frames[responseIndex];
    expect(JSON.parse(responseFrame.data)).toMatchObject({
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
        body: JSON.stringify({ content: "Generate an image of a robot panda" }),
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
    const payload = JSON.parse(imageFrame?.data ?? "{}") as {
      data_url: string;
      path?: string;
    };
    expect(payload.data_url).toMatch(/^data:image\/png;base64,/);
    expect(payload.path).toBe("workspace/generated/preview.png");
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
        body: JSON.stringify({ content: "Spawn a background job for this" }),
      }),
      state
    );
    expect(send.status).toBe(202);

    const frames = await collectSse(response, (seen) =>
      seen.some((frame) => frame.event === "response")
    );

    const jobFrame = frames.find((frame) => frame.event === "job_started");
    expect(jobFrame).toBeDefined();
    expect(JSON.parse(jobFrame?.data ?? "{}")).toMatchObject({
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
    const payload = JSON.parse(responseFrame?.data ?? "{}") as {
      content: string;
    };
    expect(payload.content).toContain("Received 2 image attachment(s).");
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
    const authBody = (await authResponse.json()) as { success: boolean };
    expect(authBody.success).toBe(true);

    const frames = await collectSse(response, (seen) =>
      seen.some((frame) => frame.event === "auth_completed")
    );
    const authFrame = frames.find((frame) => frame.event === "auth_completed");
    expect(authFrame).toBeDefined();
    expect(JSON.parse(authFrame?.data ?? "{}")).toMatchObject({
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
    const body = (await response.json()) as { success: boolean };
    expect(body.success).toBe(false);
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
    const body = (await response.json()) as { success: boolean };
    expect(body.success).toBe(true);
  });
});

describe("stub pairing contract", () => {
  it("returns an empty request list for an unknown channel", async () => {
    const state = new MockBackendState();
    const list = (await getJson(state, "/api/pairing/does-not-exist")) as {
      channel: string;
      requests: unknown[];
    };
    expect(list.channel).toBe("does-not-exist");
    expect(list.requests).toEqual([]);
  });

  it("lists the deterministic whatsapp pending pairing request", async () => {
    const state = new MockBackendState();
    const list = (await getJson(state, "/api/pairing/whatsapp")) as {
      channel: string;
      requests: { code: string; sender_id: string; created_at: string }[];
    };
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
    const body = (await approve.json()) as { success: boolean };
    expect(body.success).toBe(true);

    const list = (await getJson(state, "/api/pairing/whatsapp")) as {
      requests: unknown[];
    };
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
    const body = (await approve.json()) as { success: boolean };
    expect(body.success).toBe(false);
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
