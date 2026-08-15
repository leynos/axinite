import { deleteJson, postJson, requestJson } from "@/lib/api/client";
import type {
  ActionResponse,
  SkillInstallRequest,
  SkillListResponse,
  SkillSearchRequest,
  SkillSearchResponse,
} from "@/lib/api/contracts";

export function fetchSkills(): Promise<SkillListResponse> {
  return requestJson<SkillListResponse>("/api/skills");
}

export function searchSkills(
  request: SkillSearchRequest
): Promise<SkillSearchResponse> {
  return postJson<SkillSearchResponse>("/api/skills/search", request);
}

export function installSkill(
  request: SkillInstallRequest
): Promise<ActionResponse> {
  return postJson<ActionResponse>("/api/skills/install", request);
}

export function removeSkill(name: string): Promise<ActionResponse> {
  return deleteJson<ActionResponse>(`/api/skills/${name}`);
}
