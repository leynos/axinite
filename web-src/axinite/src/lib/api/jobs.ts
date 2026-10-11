/**
 * @file Jobs API wrapper for `/api/jobs/*`: list, summary, detail, events, project
 * files, and the restart, cancel, and prompt actions. Used by the `/jobs`
 * surface.
 */

import { postJson, requestJson } from "@/lib/api/client";
import type {
  ActionResponse,
  JobDetailResponse,
  JobEventsResponse,
  JobListResponse,
  JobPromptRequest,
  JobSummaryResponse,
  ProjectFileReadResponse,
  ProjectFilesResponse,
} from "@/lib/api/contracts";

/** GET /api/jobs: list jobs. */
export function fetchJobs(): Promise<JobListResponse> {
  return requestJson<JobListResponse>("/api/jobs");
}

/** GET /api/jobs/summary: fetch aggregate job counts. */
export function fetchJobSummary(): Promise<JobSummaryResponse> {
  return requestJson<JobSummaryResponse>("/api/jobs/summary");
}

/** GET /api/jobs/{id}: fetch one job's detail. */
export function fetchJobDetail(id: string): Promise<JobDetailResponse> {
  return requestJson<JobDetailResponse>(`/api/jobs/${encodeURIComponent(id)}`);
}

/** GET /api/jobs/{id}/events: fetch a job's events. */
export function fetchJobEvents(id: string): Promise<JobEventsResponse> {
  return requestJson<JobEventsResponse>(
    `/api/jobs/${encodeURIComponent(id)}/events`
  );
}

/** GET /api/jobs/{id}/files/list: list a job's project files. */
export function fetchJobFiles(id: string): Promise<ProjectFilesResponse> {
  return requestJson<ProjectFilesResponse>(
    `/api/jobs/${encodeURIComponent(id)}/files/list`
  );
}

/** GET /api/jobs/{id}/files/read: read one project file by `path`. */
export function readJobFile(
  id: string,
  path: string
): Promise<ProjectFileReadResponse> {
  const url = new URL(
    `/api/jobs/${encodeURIComponent(id)}/files/read`,
    window.location.origin
  );
  url.searchParams.set("path", path);
  return requestJson<ProjectFileReadResponse>(`${url.pathname}${url.search}`);
}

/** POST /api/jobs/{id}/restart: restart a job. */
export function restartJob(id: string): Promise<ActionResponse> {
  return postJson<ActionResponse>(
    `/api/jobs/${encodeURIComponent(id)}/restart`
  );
}

/** POST /api/jobs/{id}/cancel: cancel a job. */
export function cancelJob(id: string): Promise<ActionResponse> {
  return postJson<ActionResponse>(`/api/jobs/${encodeURIComponent(id)}/cancel`);
}

/** POST /api/jobs/{id}/prompt: send a follow-up prompt to a job. */
export function promptJob(
  id: string,
  request: JobPromptRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse>(
    `/api/jobs/${encodeURIComponent(id)}/prompt`,
    request
  );
}
