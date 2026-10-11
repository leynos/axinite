/**
 * @file Routines API wrapper for `/api/routines/*`: list, summary, detail, runs,
 * and the trigger, toggle, and delete actions. Used by the `/routines`
 * surface.
 */

import { deleteJson, postJson, requestJson } from "@/lib/api/client";
import type {
  ActionResponse,
  RoutineDetailResponse,
  RoutineListResponse,
  RoutineRunsResponse,
  RoutineSummaryResponse,
  ToggleRequest,
} from "@/lib/api/contracts";

/** GET /api/routines: list routines. */
export function fetchRoutines(): Promise<RoutineListResponse> {
  return requestJson<RoutineListResponse>("/api/routines");
}

/** GET /api/routines/summary: fetch aggregate routine counts. */
export function fetchRoutineSummary(): Promise<RoutineSummaryResponse> {
  return requestJson<RoutineSummaryResponse>("/api/routines/summary");
}

/** GET /api/routines/{id}: fetch one routine's detail. */
export function fetchRoutineDetail(id: string): Promise<RoutineDetailResponse> {
  return requestJson<RoutineDetailResponse>(
    `/api/routines/${encodeURIComponent(id)}`
  );
}

/** GET /api/routines/{id}/runs: list a routine's runs. */
export function fetchRoutineRuns(id: string): Promise<RoutineRunsResponse> {
  return requestJson<RoutineRunsResponse>(
    `/api/routines/${encodeURIComponent(id)}/runs`
  );
}

/** POST /api/routines/{id}/trigger: run a routine now. */
export function triggerRoutine(id: string): Promise<ActionResponse> {
  return postJson<ActionResponse>(
    `/api/routines/${encodeURIComponent(id)}/trigger`
  );
}

/** POST /api/routines/{id}/toggle: enable or disable a routine. */
export function toggleRoutine(
  id: string,
  request?: ToggleRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse>(
    `/api/routines/${encodeURIComponent(id)}/toggle`,
    request ?? {}
  );
}

/** DELETE /api/routines/{id}: delete a routine. */
export function deleteRoutine(id: string): Promise<ActionResponse> {
  return deleteJson<ActionResponse>(`/api/routines/${encodeURIComponent(id)}`);
}
