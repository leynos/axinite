/**
 * @file Route-boundary tests for malformed and invalid mock-backend request
 * bodies. Each case uses a fresh state so rejected input cannot leak between
 * routes or cases.
 */

import { describe, expect, it } from "vitest";

import { handleMockRequest } from "../../mock-backend/src/server";
import { MockBackendState } from "../../mock-backend/src/state";

type StateSnapshot = (state: MockBackendState) => string;

type InvalidRequestCase = {
  name: string;
  path: string;
  body: string;
  snapshot?: StateSnapshot;
};

function postRequest(path: string, body: string): Request {
  return new Request(`http://mock.test${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body,
  });
}

function serializeSnapshot<T extends object>(value: T): string {
  return JSON.stringify(value);
}

function observeStateEffects(state: MockBackendState) {
  const chatEvents: unknown[] = [];
  const logEntries: unknown[] = [];
  const unsubscribeChat = state.subscribeToChat({
    send: (event) => chatEvents.push(event),
    close: () => {},
  });
  const unsubscribeLogs = state.subscribeToLogs({
    send: (entry) => logEntries.push(entry),
    close: () => {},
  });
  const initialLogEntries = [...logEntries];

  return {
    assertUnchanged() {
      expect(chatEvents).toEqual([]);
      expect(logEntries).toEqual(initialLogEntries);
    },
    dispose() {
      unsubscribeChat();
      unsubscribeLogs();
    },
  };
}

const malformedJson = "{ malformed";

const invalidRequests: InvalidRequestCase[] = [
  {
    name: "approval rejects malformed JSON",
    path: "/api/chat/approval",
    body: malformedJson,
    snapshot: (state) =>
      serializeSnapshot({
        threads: state.listThreads(),
        history: state.getHistory("thread-review"),
      }),
  },
  {
    name: "approval rejects a missing request_id",
    path: "/api/chat/approval",
    body: JSON.stringify({ action: "approve" }),
    snapshot: (state) => serializeSnapshot(state.getHistory("thread-review")),
  },
  {
    name: "approval rejects a missing action",
    path: "/api/chat/approval",
    body: JSON.stringify({ request_id: "approval-review-1" }),
    snapshot: (state) => serializeSnapshot(state.getHistory("thread-review")),
  },
  {
    name: "approval rejects a non-string action",
    path: "/api/chat/approval",
    body: JSON.stringify({ request_id: "approval-review-1", action: false }),
    snapshot: (state) => serializeSnapshot(state.getHistory("thread-review")),
  },
  {
    name: "approval rejects a non-string optional thread_id",
    path: "/api/chat/approval",
    body: JSON.stringify({
      request_id: "approval-review-1",
      action: "approve",
      thread_id: 7,
    }),
    snapshot: (state) => serializeSnapshot(state.getHistory("thread-review")),
  },
  {
    name: "auth token rejects malformed JSON",
    path: "/api/chat/auth-token",
    body: malformedJson,
  },
  {
    name: "auth token rejects a missing extension_name",
    path: "/api/chat/auth-token",
    body: JSON.stringify({ token: "valid-token" }),
  },
  {
    name: "auth token rejects a missing token",
    path: "/api/chat/auth-token",
    body: JSON.stringify({ extension_name: "google-drive" }),
  },
  {
    name: "auth token rejects a non-string extension_name",
    path: "/api/chat/auth-token",
    body: JSON.stringify({ extension_name: 3, token: "valid-token" }),
  },
  {
    name: "auth cancellation rejects malformed JSON",
    path: "/api/chat/auth-cancel",
    body: malformedJson,
  },
  {
    name: "auth cancellation rejects a missing extension_name",
    path: "/api/chat/auth-cancel",
    body: "{}",
  },
  {
    name: "auth cancellation rejects a non-string extension_name",
    path: "/api/chat/auth-cancel",
    body: JSON.stringify({ extension_name: false }),
  },
  {
    name: "pairing approval rejects malformed JSON",
    path: "/api/pairing/whatsapp/approve",
    body: malformedJson,
    snapshot: (state) => serializeSnapshot(state.pairingList("whatsapp")),
  },
  {
    name: "pairing approval rejects a missing code",
    path: "/api/pairing/whatsapp/approve",
    body: "{}",
    snapshot: (state) => serializeSnapshot(state.pairingList("whatsapp")),
  },
  {
    name: "pairing approval rejects a non-string code",
    path: "/api/pairing/whatsapp/approve",
    body: JSON.stringify({ code: 1234 }),
    snapshot: (state) => serializeSnapshot(state.pairingList("whatsapp")),
  },
  {
    name: "memory write rejects malformed JSON",
    path: "/api/memory/write",
    body: malformedJson,
    snapshot: (state) =>
      serializeSnapshot({
        tree: state.getMemoryTree(),
        document: state.readMemory("workspace/AGENTS.md"),
      }),
  },
  {
    name: "memory write rejects a missing path",
    path: "/api/memory/write",
    body: JSON.stringify({ content: "replacement" }),
    snapshot: (state) => serializeSnapshot(state.getMemoryTree()),
  },
  {
    name: "memory write rejects a missing content field",
    path: "/api/memory/write",
    body: JSON.stringify({ path: "workspace/AGENTS.md" }),
    snapshot: (state) =>
      serializeSnapshot(state.readMemory("workspace/AGENTS.md")),
  },
  {
    name: "memory write rejects non-string content",
    path: "/api/memory/write",
    body: JSON.stringify({ path: "workspace/AGENTS.md", content: 9 }),
    snapshot: (state) =>
      serializeSnapshot(state.readMemory("workspace/AGENTS.md")),
  },
  {
    name: "memory write rejects a non-string path",
    path: "/api/memory/write",
    body: JSON.stringify({ path: 9, content: "replacement" }),
    snapshot: (state) => serializeSnapshot(state.getMemoryTree()),
  },
  {
    name: "memory search rejects malformed JSON",
    path: "/api/memory/search",
    body: malformedJson,
  },
  {
    name: "memory search rejects a missing query",
    path: "/api/memory/search",
    body: "{}",
  },
  {
    name: "memory search rejects a non-string query",
    path: "/api/memory/search",
    body: JSON.stringify({ query: 1 }),
  },
  {
    name: "memory search rejects a non-number optional limit",
    path: "/api/memory/search",
    body: JSON.stringify({ query: "workspace", limit: "2" }),
  },
  {
    name: "job prompt rejects malformed JSON",
    path: "/api/jobs/job-audit/prompt",
    body: malformedJson,
    snapshot: (state) => serializeSnapshot(state.getJobEvents("job-audit")),
  },
  {
    name: "job prompt rejects a missing content field",
    path: "/api/jobs/job-audit/prompt",
    body: "{}",
    snapshot: (state) => serializeSnapshot(state.getJobEvents("job-audit")),
  },
  {
    name: "job prompt rejects non-string content",
    path: "/api/jobs/job-audit/prompt",
    body: JSON.stringify({ content: 42 }),
    snapshot: (state) => serializeSnapshot(state.getJobEvents("job-audit")),
  },
  {
    name: "job prompt rejects non-boolean optional done",
    path: "/api/jobs/job-audit/prompt",
    body: JSON.stringify({ content: "follow up", done: "yes" }),
    snapshot: (state) => serializeSnapshot(state.getJobEvents("job-audit")),
  },
  {
    name: "extension setup rejects malformed JSON",
    path: "/api/extensions/jmap/setup",
    body: malformedJson,
    snapshot: (state) =>
      serializeSnapshot({
        setup: state.getExtensionSetup("jmap"),
        extensions: state.listExtensions(),
      }),
  },
  {
    name: "extension setup rejects a missing secrets field",
    path: "/api/extensions/jmap/setup",
    body: "{}",
    snapshot: (state) => serializeSnapshot(state.getExtensionSetup("jmap")),
  },
  {
    name: "extension setup rejects a non-object secrets field",
    path: "/api/extensions/jmap/setup",
    body: JSON.stringify({ secrets: [] }),
    snapshot: (state) => serializeSnapshot(state.getExtensionSetup("jmap")),
  },
  {
    name: "extension setup rejects non-string secret values",
    path: "/api/extensions/jmap/setup",
    body: JSON.stringify({ secrets: { username: 12 } }),
    snapshot: (state) => serializeSnapshot(state.getExtensionSetup("jmap")),
  },
  {
    name: "skill search rejects malformed JSON",
    path: "/api/skills/search",
    body: malformedJson,
  },
  {
    name: "skill search rejects a missing query",
    path: "/api/skills/search",
    body: "{}",
  },
  {
    name: "skill search rejects a non-string query",
    path: "/api/skills/search",
    body: JSON.stringify({ query: null }),
  },
  {
    name: "skill install rejects malformed JSON",
    path: "/api/skills/install",
    body: malformedJson,
    snapshot: (state) => serializeSnapshot(state.listSkills()),
  },
  {
    name: "skill install rejects a missing name",
    path: "/api/skills/install",
    body: JSON.stringify({ slug: "catalog/react-patterns" }),
    snapshot: (state) => serializeSnapshot(state.listSkills()),
  },
  {
    name: "skill install rejects a non-string name",
    path: "/api/skills/install",
    body: JSON.stringify({ name: 12 }),
    snapshot: (state) => serializeSnapshot(state.listSkills()),
  },
  {
    name: "skill install rejects a non-string optional slug",
    path: "/api/skills/install",
    body: JSON.stringify({ name: "react_patterns", slug: 5 }),
    snapshot: (state) => serializeSnapshot(state.listSkills()),
  },
  {
    name: "skill install rejects a non-string optional url",
    path: "/api/skills/install",
    body: JSON.stringify({ name: "local-preview", url: false }),
    snapshot: (state) => serializeSnapshot(state.listSkills()),
  },
  {
    name: "skill install rejects a non-string optional content field",
    path: "/api/skills/install",
    body: JSON.stringify({ name: "local-preview", content: false }),
    snapshot: (state) => serializeSnapshot(state.listSkills()),
  },
];

describe("mock-backend request validation", () => {
  it.each(invalidRequests)("$name", async ({ path, body, snapshot }) => {
    const state = new MockBackendState();
    const before = snapshot?.(state);
    const effects = observeStateEffects(state);

    try {
      const response = await handleMockRequest(postRequest(path, body), state);

      expect(response.status).toBe(400);
      effects.assertUnchanged();
      if (snapshot) {
        expect(snapshot(state)).toBe(before);
      }
    } finally {
      effects.dispose();
      state.dispose();
    }
  });
});
