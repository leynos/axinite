/** @file Mock queries return detached DTOs and cannot issue state commands. */

import { describe, expect, it } from "vitest";

import { parseMockFeatureFlagEnvironment } from "../../mock-backend/src/flag-environment";
import { MockBackendState } from "../../mock-backend/src/state";

/** Fail clearly when a seeded collection needed by a query is missing. */
function first<T>(entries: readonly T[]): T {
  const entry = entries[0];
  if (entry === undefined) throw new Error("Expected a seeded query fixture");
  return entry;
}

/** Simulate an in-process consumer changing every mutable part of a DTO. */
function corrupt(value: unknown): void {
  if (typeof value !== "object" || value === null) return;
  for (const [key, entry] of Object.entries(value)) {
    if (typeof entry === "object" && entry !== null) {
      corrupt(entry);
    } else {
      Reflect.set(value, key, "consumer changed this value");
    }
  }
  if (Array.isArray(value)) value.push({ injected: true });
}

const queries: { name: string; read: (state: MockBackendState) => unknown }[] =
  [
    { name: "status", read: (state) => state.getGatewayStatus() },
    { name: "flags", read: (state) => state.getFeatureFlags() },
    { name: "threads", read: (state) => state.listThreads() },
    { name: "history", read: (state) => state.getHistory("thread-assistant") },
    { name: "pairing", read: (state) => state.pairingList("whatsapp") },
    { name: "memory tree", read: (state) => state.getMemoryTree() },
    { name: "memory listing", read: (state) => state.listMemory() },
    {
      name: "memory read",
      read: (state) =>
        state.readMemory(
          first(state.getMemoryTree().entries.filter((entry) => !entry.is_dir))
            .path
        ),
    },
    {
      name: "memory search",
      read: (state) => state.searchMemory({ query: "a" }),
    },
    { name: "jobs", read: (state) => state.listJobs() },
    { name: "job summary", read: (state) => state.summarizeJobs() },
    { name: "job detail", read: (state) => state.getJob("job-comparison") },
    {
      name: "job events",
      read: (state) => state.getJobEvents("job-comparison"),
    },
    {
      name: "job files",
      read: (state) => state.listJobFiles("job-comparison"),
    },
    {
      name: "job file read",
      read: (state) =>
        state.readJobFile(
          "job-comparison",
          first(state.listJobFiles("job-comparison").entries).path
        ),
    },
    { name: "routines", read: (state) => state.listRoutines() },
    { name: "routine summary", read: (state) => state.summarizeRoutines() },
    {
      name: "routine detail",
      read: (state) =>
        state.getRoutine(first(state.listRoutines().routines).id),
    },
    {
      name: "routine runs",
      read: (state) =>
        state.getRoutineRuns(first(state.listRoutines().routines).id),
    },
    { name: "extensions", read: (state) => state.listExtensions() },
    { name: "extension tools", read: (state) => state.listExtensionTools() },
    {
      name: "extension registry",
      read: (state) => state.searchExtensionRegistry(),
    },
    {
      name: "extension setup",
      read: (state) =>
        state.getExtensionSetup(first(state.listExtensions().extensions).name),
    },
    { name: "skills", read: (state) => state.listSkills() },
    {
      name: "skill catalogue",
      read: (state) => state.searchSkills({ query: "" }),
    },
    { name: "log level", read: (state) => state.getLogLevel() },
  ];

describe("mock query purity", () => {
  it.each(
    queries
  )("$name returns a detached snapshot without selecting a thread", ({
    read,
  }) => {
    const state = new MockBackendState();
    const active = state.listThreads().active_thread;
    const response = read(state);
    const expected = structuredClone(response);
    expect(state.listThreads().active_thread).toBe(active);
    corrupt(response);
    expect(read(state)).toEqual(expected);
    expect(state.listThreads().active_thread).toBe(active);
  });

  it("keeps history reads separate from explicit create and send selection", () => {
    const state = new MockBackendState();
    const thread = state.createThread();
    state.getHistory("thread-assistant");
    expect(state.listThreads().active_thread).toBe(thread.id);
    state.dispose();
  });

  it("does not expose the created thread as a mutable state alias", () => {
    const state = new MockBackendState();
    const thread = state.createThread();
    const expected = structuredClone(thread);
    corrupt(thread);
    expect(state.listThreads().threads).toContainEqual(expected);
  });

  it("copies the injected flag overlay and ignores unknown names", () => {
    const overlay = { route_jobs: false, unregistered: true };
    const state = new MockBackendState(undefined, {
      featureFlagOverrides: overlay,
    });
    overlay.route_jobs = true;
    expect(state.getFeatureFlags()["route_jobs"]).toBe(false);
    expect(state.getFeatureFlags()).not.toHaveProperty("unregistered");
  });

  it("parses environment at composition without retaining a mutable dependency", () => {
    const environment = {
      FEATURE_FLAG_ROUTE_CHAT: "TRUE",
      FEATURE_FLAG_PANEL_LOGS: "0",
      OTHER: "true",
    };
    const overlay = parseMockFeatureFlagEnvironment(environment);
    const state = new MockBackendState(undefined, {
      featureFlagOverrides: overlay,
    });
    environment.FEATURE_FLAG_PANEL_LOGS = "true";
    expect(overlay).toEqual({ route_chat: true, panel_logs: false });
    expect(state.getFeatureFlags()["panel_logs"]).toBe(false);
    expect(state.getFeatureFlags()).toEqual(state.getFeatureFlags());
  });
});
