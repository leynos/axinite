import { deleteJson, postJson, requestJson } from "@/lib/api/client";
import type {
  ActionResponse,
  RoutineDetailResponse,
  RoutineListResponse,
  RoutineRunsResponse,
  RoutineSummaryResponse,
  ToggleRequest,
} from "@/lib/api/contracts";

export function fetchRoutines(): Promise<RoutineListResponse> {
  return requestJson<RoutineListResponse>("/api/routines");
}

export function fetchRoutineSummary(): Promise<RoutineSummaryResponse> {
  return requestJson<RoutineSummaryResponse>("/api/routines/summary");
}

export function fetchRoutineDetail(id: string): Promise<RoutineDetailResponse> {
  return requestJson<RoutineDetailResponse>(`/api/routines/${id}`);
}

export function fetchRoutineRuns(id: string): Promise<RoutineRunsResponse> {
  return requestJson<RoutineRunsResponse>(`/api/routines/${id}/runs`);
}

export function triggerRoutine(id: string): Promise<ActionResponse> {
  return postJson<ActionResponse>(`/api/routines/${id}/trigger`);
}

export function toggleRoutine(
  id: string,
  request?: ToggleRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse>(`/api/routines/${id}/toggle`, request ?? {});
}

export function deleteRoutine(id: string): Promise<ActionResponse> {
  return deleteJson<ActionResponse>(`/api/routines/${id}`);
}
