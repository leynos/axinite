/**
 * @file Routine mutation contracts for the stub HTTP adapter. Reads after each
 * action verify that its acknowledgement corresponds to persisted mock state.
 */

import { describe, expect, it } from "vitest";

import type {
  ActionResponse,
  RoutineDetailResponse,
  RoutineListResponse,
  RoutineRunsResponse,
  RoutineSummaryResponse,
} from "@/lib/api/contracts";
import { handleMockRequest } from "../../mock-backend/src/server";
import { MockBackendState } from "../../mock-backend/src/state";

async function requestJson<T>(
  state: MockBackendState,
  path: string,
  method = "GET",
  body?: unknown
): Promise<T> {
  const response = await handleMockRequest(
    new Request(`http://mock.test${path}`, {
      method,
      headers: { "Content-Type": "application/json" },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    }),
    state
  );
  expect(response.status).toBe(200);
  return response.json() as Promise<T>;
}

async function firstRoutine(state: MockBackendState): Promise<string> {
  const list = await requestJson<RoutineListResponse>(state, "/api/routines");
  const id = list.routines[0]?.id;
  expect(id).toBeDefined();
  if (!id) {
    throw new Error("Seed routines must contain a routine");
  }
  return id;
}

describe("stub routine mutation contracts", () => {
  it("records a triggered run in the detail, list and run history", async () => {
    const state = new MockBackendState();
    const id = await firstRoutine(state);
    const path = `/api/routines/${id}`;
    const before = await requestJson<RoutineDetailResponse>(state, path);
    const action = await requestJson<ActionResponse>(
      state,
      `${path}/trigger`,
      "POST"
    );
    expect(action.success).toBe(true);

    const detail = await requestJson<RoutineDetailResponse>(state, path);
    const runs = await requestJson<RoutineRunsResponse>(state, `${path}/runs`);
    const list = await requestJson<RoutineListResponse>(state, "/api/routines");
    expect(detail.run_count).toBe(before.run_count + 1);
    expect(runs.runs[0]).toMatchObject({
      trigger_type: "manual",
      status: "completed",
      result_summary: "Manual trigger completed in the preview shell.",
      started_at: detail.last_run_at,
    });
    expect(detail.recent_runs).toEqual(runs.runs);
    expect(list.routines.find((routine) => routine.id === id)).toMatchObject({
      run_count: detail.run_count,
      last_run_at: detail.last_run_at,
    });
  });

  it.each([
    false,
    true,
  ])("persists enabled=%s in detail, list and summary", async (enabled) => {
    const state = new MockBackendState();
    const id = await firstRoutine(state);
    const path = `/api/routines/${id}`;
    const before = await requestJson<RoutineSummaryResponse>(
      state,
      "/api/routines/summary"
    );
    const previous = await requestJson<RoutineDetailResponse>(state, path);
    const action = await requestJson<ActionResponse>(
      state,
      `${path}/toggle`,
      "POST",
      { enabled }
    );
    expect(action.success).toBe(true);

    expect(await requestJson<RoutineDetailResponse>(state, path)).toMatchObject(
      { enabled }
    );
    const list = await requestJson<RoutineListResponse>(state, "/api/routines");
    expect(list.routines.find((routine) => routine.id === id)?.enabled).toBe(
      enabled
    );
    const summary = await requestJson<RoutineSummaryResponse>(
      state,
      "/api/routines/summary"
    );
    const delta = Number(enabled) - Number(previous.enabled);
    expect(summary.enabled).toBe(before.enabled + delta);
    expect(summary.disabled).toBe(before.disabled - delta);
  });

  it("toggles twice without an explicit enabled value", async () => {
    const state = new MockBackendState();
    const id = await firstRoutine(state);
    const path = `/api/routines/${id}`;
    const before = await requestJson<RoutineDetailResponse>(state, path);
    for (const enabled of [!before.enabled, before.enabled]) {
      expect(
        (await requestJson<ActionResponse>(state, `${path}/toggle`, "POST"))
          .success
      ).toBe(true);
      expect(
        (await requestJson<RoutineDetailResponse>(state, path)).enabled
      ).toBe(enabled);
    }
  });

  it("removes the deleted routine from list and summary and rejects detail reads", async () => {
    const state = new MockBackendState();
    const id = await firstRoutine(state);
    const before = await requestJson<RoutineSummaryResponse>(
      state,
      "/api/routines/summary"
    );
    const action = await requestJson<ActionResponse>(
      state,
      `/api/routines/${id}`,
      "DELETE"
    );
    expect(action.success).toBe(true);

    const list = await requestJson<RoutineListResponse>(state, "/api/routines");
    expect(list.routines.some((routine) => routine.id === id)).toBe(false);
    const summary = await requestJson<RoutineSummaryResponse>(
      state,
      "/api/routines/summary"
    );
    expect(summary.total).toBe(before.total - 1);
    const detail = await handleMockRequest(
      new Request(`http://mock.test/api/routines/${id}`),
      state
    );
    expect(detail.ok).toBe(false);
    expect(await detail.text()).toContain("Routine not found");
  });
});
