/**
 * Jobs API wrapper for `/api/jobs/*`: list, summary, detail, events, project
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

export function fetchJobs(): Promise<JobListResponse> {
  return requestJson<JobListResponse>("/api/jobs");
}

export function fetchJobSummary(): Promise<JobSummaryResponse> {
  return requestJson<JobSummaryResponse>("/api/jobs/summary");
}

export function fetchJobDetail(id: string): Promise<JobDetailResponse> {
  return requestJson<JobDetailResponse>(`/api/jobs/${encodeURIComponent(id)}`);
}

export function fetchJobEvents(id: string): Promise<JobEventsResponse> {
  return requestJson<JobEventsResponse>(
    `/api/jobs/${encodeURIComponent(id)}/events`
  );
}

export function fetchJobFiles(id: string): Promise<ProjectFilesResponse> {
  return requestJson<ProjectFilesResponse>(
    `/api/jobs/${encodeURIComponent(id)}/files/list`
  );
}

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

export function restartJob(id: string): Promise<ActionResponse> {
  return postJson<ActionResponse>(
    `/api/jobs/${encodeURIComponent(id)}/restart`
  );
}

export function cancelJob(id: string): Promise<ActionResponse> {
  return postJson<ActionResponse>(`/api/jobs/${encodeURIComponent(id)}/cancel`);
}

export function promptJob(
  id: string,
  request: JobPromptRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse>(
    `/api/jobs/${encodeURIComponent(id)}/prompt`,
    request
  );
}
